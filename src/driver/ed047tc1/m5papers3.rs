use esp_hal::gpio::{Level, Output, OutputConfig, OutputPin};
use esp_hal::time::Instant;

use super::ed047tc1::{DrawMode, Ed047tc1, HEIGHT, PinConfig, RowData};

/// M5PaperS3 - ED047TC1 驱动实现
///
/// 显示流程：
/// 1. 刷一屏 -> 发一次 SPV
/// 2. 刷一行 -> 发一次 XSTL + 发数据 + XLE 锁存 + CKV 换行
/// 3. 不用屏幕时拉低 PWR 省电
pub struct M5PaperS3<'d> {
    /// 核心驱动
    pub core: Ed047tc1,
    /// 8 条数据线，电子纸是4bit 16 级灰度，所以一次传 2 个像素。
    /// - 用途：传像素灰度数据
    /// - 用法：要显示什么颜色，就把 8bit 数据放到这 8 根线上。
    data: [Output<'d>; 8],
    /// 行起始脉冲
    /// - 用途：告诉屏幕，这一行数据开始传！
    /// - 用法：每一行开始前发一次，拉低 1us -> 再拉高。
    xstl: Output<'d>,
    /// 行锁存
    /// - 用途：数据传完了，锁进去！
    /// - 用法：一行数据发完 -> XLE 脉冲一下，数据就被屏幕锁存并显示。
    xle: Output<'d>,
    /// 帧起始脉冲
    /// - 用途：告诉屏幕：一帧画面要开始了！
    /// - 用法：刷一整屏 -> 先拉低 1us -> 再拉高
    /// 只发一次
    spv: Output<'d>,
    /// 行扫描时钟
    /// - 用途：切换到下一行
    /// - 用法：一行传完 -> 翻转一次 CKV，屏幕就会往下走一行。
    ckv: Output<'d>,
    /// 电源
    /// - 用途：控制屏幕电源
    /// - 用法：拉高 = 开机，拉低 = 关机/省电
    pwr: Output<'d>,
}

impl<'d> M5PaperS3<'d> {
    /// 创建 ESP32-S3 平台的 ED047TC1 驱动
    ///
    /// # 参数
    /// * `data0` - 数据线 D0 (G6)
    /// * `data1` - 数据线 D1 (G14)
    /// * `data2` - 数据线 D2 (G7)
    /// * `data3` - 数据线 D3 (G12)
    /// * `data4` - 数据线 D4 (G9)
    /// * `data5` - 数据线 D5 (G11)
    /// * `data6` - 数据线 D6 (G8)
    /// * `data7` - 数据线 D7 (G10)
    /// * `xstl`  - 行起始脉冲 (G13)
    /// * `xle`   - 行锁存 (G15)
    /// * `spv`   - 帧起始脉冲 (G17)
    /// * `ckv`   - 垂直扫描时钟 (G18)
    /// * `pwr`   - 屏幕上电控制 (G45)
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        data0: impl OutputPin + 'd,
        data1: impl OutputPin + 'd,
        data2: impl OutputPin + 'd,
        data3: impl OutputPin + 'd,
        data4: impl OutputPin + 'd,
        data5: impl OutputPin + 'd,
        data6: impl OutputPin + 'd,
        data7: impl OutputPin + 'd,
        xstl: impl OutputPin + 'd,
        xle: impl OutputPin + 'd,
        spv: impl OutputPin + 'd,
        ckv: impl OutputPin + 'd,
        pwr: impl OutputPin + 'd,
    ) -> Self {
        let output_config = OutputConfig::default();
        Self {
            /*core: Ed047tc1::new(PinConfig {
                data0,
                data1,
                data2,
                data3,
                data4,
                data5,
                data6,
                data7,
                xstl,
                xle,
                spv,
                ckv,
                pwr,
            }),*/
            data: [
                Output::new(data0, Level::Low, output_config),
                Output::new(data1, Level::Low, output_config),
                Output::new(data2, Level::Low, output_config),
                Output::new(data3, Level::Low, output_config),
                Output::new(data4, Level::Low, output_config),
                Output::new(data5, Level::Low, output_config),
                Output::new(data6, Level::Low, output_config),
                Output::new(data7, Level::Low, output_config),
            ],
            xstl: Output::new(xstl, Level::Low, output_config),
            xle: Output::new(xle, Level::Low, output_config),
            spv: Output::new(spv, Level::Low, output_config),
            ckv: Output::new(ckv, Level::Low, output_config),
            pwr: Output::new(pwr, Level::Low, output_config),
        }
    }

    /// 初始化显示屏
    pub fn init(&mut self) {
        // 打开屏幕电源
        self.pwr.set_low();
        // 设置初始状态
        self.xstl.set_low();
        self.xle.set_low();
        self.spv.set_low();
        self.ckv.set_high();

        // 清除数据线
        for pin in &mut self.data {
            pin.set_low();
        }
    }

    /// 关闭显示屏电源
    pub fn power_off(&mut self) {
        self.pwr.set_low();
    }

    /// 打开显示屏电源
    pub fn power_on(&mut self) {
        self.pwr.set_high();
        self.delay_us(1000);
    }

    /// 刷新显示屏
    ///
    /// # 参数
    /// * `mode` - 刷新模式
    pub fn refresh(&mut self, mode: DrawMode) {
        // 获取波形相位数据
        let waveform_phase = match self.core.get_waveform_phase(mode) {
            Some(phase) => phase,
            None => return,
        };

        let phase_count = waveform_phase.phase as usize;

        // 开始刷新
        self.start_frame();

        // 每个相位循环
        for phase_idx in 0..phase_count {
            let phase_time = waveform_phase.time[phase_idx];

            // 逐行刷新
            for row in 0..HEIGHT {
                // 构建行数据
                let mut row_data = RowData::new();
                row_data.build_from_buffer(
                    self.core.frame_buffer(),
                    &self.core.prev_buffer,
                    row,
                    phase_idx,
                    waveform_phase,
                );

                // 输出行数据
                self.output_row(&row_data, row);

                // 相位时间延时 (时间单位: 1/10 微秒)
                if phase_time > 0 {
                    self.delay_us((phase_time / 10) as u32);
                }
            }
        }

        // 结束刷新
        self.end_frame();

        // 复制当前帧到上一帧（为下次差分刷新做准备）
        self.core.copy_to_prev();
    }

    /// 清屏为白色并刷新
    pub fn clear_white(&mut self) {
        self.core.clear_white();
        self.refresh(DrawMode::Gc16);
    }

    /// 清屏为黑色并刷新
    pub fn clear_black(&mut self) {
        self.core.clear_black();
        self.refresh(DrawMode::Gc16);
    }

    /// 开始帧传输
    fn start_frame(&mut self) {
        // 使能输出
        self.oe.set_high();
        // 设置模式
        self.mode.set_high();

        self.delay_us(10);
    }

    /// 结束帧传输
    fn end_frame(&mut self) {
        // 关闭输出
        self.oe.set_low();
        // 清除模式
        self.mode.set_low();
    }

    /// 输出一行数据
    fn output_row(&mut self, row_data: &RowData, row: u32) {
        // 起始脉冲（行开始）
        if row == 0 {
            self.sph.set_low();
        }

        // 逐字节输出
        for &byte in &row_data.data {
            self.write_byte(byte);
        }

        // 恢复起始脉冲
        if row == 0 {
            self.sph.set_high();
        }

        // 锁存数据
        self.latch_row();

        // 行时钟脉冲
        self.row_pulse();
    }

    /// 写入一个字节到数据线
    fn write_byte(&mut self, byte: u8) {
        // 设置数据线
        for (i, pin) in self.data.iter_mut().enumerate() {
            if (byte >> i) & 1 == 1 {
                pin.set_high();
            } else {
                pin.set_low();
            }
        }

        // 时钟脉冲
        self.clk.set_high();
        // 短暂延时
        self.delay_us(1);
        self.clk.set_low();
    }

    /// 锁存行数据
    fn latch_row(&mut self) {
        self.stl.set_high();
        self.delay_us(1);
        self.stl.set_low();
    }

    /// 行时钟脉冲
    fn row_pulse(&mut self) {
        self.ckv.set_high();
        self.delay_us(1);
        self.ckv.set_low();
    }

    /// 微秒级延时
    #[inline]
    fn delay_us(&self, us: u32) {
        let start = Instant::now();
        while start.elapsed().as_micros() < us as u64 {}
    }
}
