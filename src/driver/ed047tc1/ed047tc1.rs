//! ED047TC1 电子墨水屏驱动实现
//!
//! 本模块实现了 ED047TC1 4.7 英寸 16 级灰度电子墨水屏的完整驱动。
//!
//! # 使用示例
//!
//! ```ignore
//! use paper_s3::driver::ed047tc1::{Ed047tc1, DrawMode, WIDTH, HEIGHT};
//!
//! // 创建驱动实例
//! let mut display = Ed047tc1::new();
//!
//! // 初始化显示屏
//! display.init(&mut gpio_pins)?;
//!
//! // 清屏
//! display.clear()?;
//!
//! // 设置像素
//! display.set_pixel(100, 100, 0x0F); // 白色
//!
//! // 刷新显示
//! display.refresh(DrawMode::Gc16)?;
//! ```

use super::waveform::{ED047TC1, Waveform, WaveformPhase};
use super::{BUFFER_SIZE, DrawMode, HEIGHT, WIDTH};

// ============================================================================
// 错误类型
// ============================================================================

/// ED047TC1 驱动错误
#[derive(Eq, Copy, Clone, Debug, PartialEq)]
pub enum Error<E> {
    /// GPIO 错误
    Gpio(E),
    /// 坐标超出范围
    OutOfBounds,
    /// 未初始化
    NotInitialized,
    /// 波形数据无效
    InvalidWaveform,
    /// 刷新超时
    RefreshTimeout,
}

// ============================================================================
// 灰度常量
// ============================================================================

/// 黑色（灰度级 0）
pub const GRAYSCALE_BLACK: u8 = 0x00;
/// 白色（灰度级 15）
pub const GRAYSCALE_WHITE: u8 = 0x0F;

// ============================================================================
// 引脚配置
// ============================================================================

/// ED047TC1 引脚配置
///
/// 定义了显示屏与 ESP32-S3 连接的 GPIO 引脚
#[derive(Debug, Clone, Copy)]
pub struct PinConfig {
    /// 数据线 D0-D7 起始引脚
    pub data_start: u8,
    /// 时钟引脚
    pub clk: u8,
    /// 数据锁存引脚
    pub stl: u8,
    /// 行选择时钟引脚
    pub ckv: u8,
    /// 电源使能引脚
    pub power_enable: u8,
    /// 模式切换引脚
    pub mode: u8,
    /// 起始脉冲引脚
    pub sph: u8,
    /// 输出使能引脚
    pub oe: u8,
}

impl Default for PinConfig {
    /// M5Paper S3 的默认引脚配置
    fn default() -> Self {
        Self {
            data_start: 6,    // GPIO6-GPIO13 为数据线
            clk: 18,          // 时钟
            stl: 4,           // 数据锁存
            ckv: 5,           // 行选择时钟
            power_enable: 38, // 电源使能
            mode: 39,         // 模式
            sph: 40,          // 起始脉冲
            oe: 45,           // 输出使能
        }
    }
}

// ============================================================================
// 显示状态
// ============================================================================

/// 显示屏状态
#[derive(Eq, Copy, Clone, Debug, Default, PartialEq)]
pub enum DisplayState {
    /// 未初始化
    #[default]
    Uninitialized,
    /// 空闲
    Idle,
    /// 刷新中
    Refreshing,
    /// 休眠
    Sleep,
}

// ============================================================================
// 帧缓冲区
// ============================================================================

/// 帧缓冲区
///
/// 每个像素使用 4 位表示灰度（0-15），两个像素打包在一个字节中（一字节是八位）。
/// 高 4 位存储偶数像素，低 4 位存储奇数像素。
pub struct FrameBuffer {
    /// 缓冲区数据
    data: [u8; BUFFER_SIZE],
}

impl FrameBuffer {
    /// 创建新的帧缓冲区（默认为白色）
    pub const fn new() -> Self {
        Self {
            // 0xFF = 两个白色像素
            data: [0xFF; BUFFER_SIZE],
        }
    }

    /// 创建黑色帧缓冲区
    pub const fn black() -> Self {
        Self {
            // 0x00 = 两个黑色像素
            data: [0x00; BUFFER_SIZE],
        }
    }

    /// 清空为指定灰度
    pub fn clear(&mut self, grayscale: u8) {
        let value = (grayscale << 4) | (grayscale & 0x0F);
        self.data.fill(value);
    }

    /// 获取像素灰度值
    ///
    /// # 参数
    /// * `x` - X 坐标（0 到 WIDTH - 1）
    /// * `y` - Y 坐标（0 到 HEIGHT - 1）
    ///
    /// # 返回值
    /// 灰度值（0-15）
    #[inline]
    pub fn get_pixel(&self, x: u32, y: u32) -> u8 {
        if x >= WIDTH || y >= HEIGHT {
            return 0;
        }

        let index = (y * WIDTH + x) as usize;
        let byte_index = index / 2;
        let byte = self.data[byte_index];

        if index % 2 == 0 {
            // 偶数像素在高 4 位
            (byte >> 4) & 0x0F
        } else {
            // 奇数像素在低 4 位
            byte & 0x0F
        }
    }

    /// 设置像素灰度值
    ///
    /// # 参数
    /// * `x` - X 坐标（0 到 WIDTH-1）
    /// * `y` - Y 坐标（0 到 HEIGHT-1）
    /// * `grayscale` - 灰度值（0-15）
    #[inline]
    pub fn set_pixel(&mut self, x: u32, y: u32, grayscale: u8) {
        if x >= WIDTH || y >= HEIGHT {
            return;
        }

        let grayscale = grayscale & 0x0F; // 限制为 4 位
        let index = (y * WIDTH + x) as usize;
        let byte_index = index / 2;

        if index % 2 == 0 {
            // 偶数像素在高 4 位
            self.data[byte_index] = (self.data[byte_index] & 0x0F) | (grayscale << 4);
        } else {
            // 奇数像素在低 4 位
            self.data[byte_index] = (self.data[byte_index] & 0xF0) | grayscale;
        }
    }

    /// 填充矩形区域
    ///
    /// # 参数
    /// * `x` - 左上角 X 坐标
    /// * `y` - 左上角 Y 坐标
    /// * `width` - 宽度
    /// * `height` - 高度
    /// * `grayscale` - 灰度值（0-15）
    pub fn fill_rect(&mut self, x: u32, y: u32, width: u32, height: u32, grayscale: u8) {
        let x_end = (x + width).min(WIDTH);
        let y_end = (y + height).min(HEIGHT);

        for py in y..y_end {
            for px in x..x_end {
                self.set_pixel(px, py, grayscale);
            }
        }
    }

    /// 绘制水平线
    pub fn draw_h_line(&mut self, x: u32, y: u32, length: u32, grayscale: u8) {
        let x_end = (x + length).min(WIDTH);
        for px in x..x_end {
            self.set_pixel(px, y, grayscale);
        }
    }

    /// 绘制垂直线
    pub fn draw_v_line(&mut self, x: u32, y: u32, length: u32, grayscale: u8) {
        let y_end = (y + length).min(HEIGHT);
        for py in y..y_end {
            self.set_pixel(x, py, grayscale);
        }
    }

    /// 获取原始缓冲区数据
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    /// 获取可变原始缓冲区数据
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data
    }
}

impl Default for FrameBuffer {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// ED047TC1 驱动
// ============================================================================

/// ED047TC1 电子墨水屏驱动
///
/// 驱动实现了以下功能：
/// - 16 级灰度显示
/// - 多种刷新模式（DU、GC16、GL16 等）
/// - 局部刷新
/// - 温度自适应波形
pub struct Ed047tc1 {
    /// 当前帧缓冲区
    frame_buffer: FrameBuffer,
    /// 上一帧缓冲区（用于差分刷新）
    prev_buffer: FrameBuffer,
    /// 显示状态
    state: DisplayState,
    /// 当前绘图模式
    draw_mode: DrawMode,
    /// 当前温度（摄氏度）
    temperature: i8,
    /// 引脚配置
    #[allow(dead_code)]
    pin_config: PinConfig,
    /// 波形配置引用
    waveform: &'static Waveform,
}

impl Ed047tc1 {
    /// 创建新的 ED047TC1 驱动实例
    ///
    /// 使用默认引脚配置和 GC16 绘图模式
    pub const fn new() -> Self {
        Self {
            frame_buffer: FrameBuffer::new(),
            prev_buffer: FrameBuffer::new(),
            state: DisplayState::Uninitialized,
            draw_mode: DrawMode::Gc16,
            temperature: 24, // 默认室温
            pin_config: PinConfig {
                data_start: 6,
                clk: 18,
                stl: 4,
                ckv: 5,
                power_enable: 38,
                mode: 39,
                sph: 40,
                oe: 45,
            },
            waveform: &ED047TC1,
        }
    }

    /// 使用自定义引脚配置创建驱动实例
    pub const fn with_pins(config: PinConfig) -> Self {
        Self {
            frame_buffer: FrameBuffer::new(),
            prev_buffer: FrameBuffer::new(),
            state: DisplayState::Uninitialized,
            draw_mode: DrawMode::Gc16,
            temperature: 24,
            pin_config: config,
            waveform: &ED047TC1,
        }
    }

    /// 获取显示屏宽度
    pub const fn width(&self) -> u32 {
        WIDTH
    }

    /// 获取显示屏高度
    pub const fn height(&self) -> u32 {
        HEIGHT
    }

    /// 获取当前显示状态
    pub const fn state(&self) -> DisplayState {
        self.state
    }

    /// 获取当前绘图模式
    pub const fn draw_mode(&self) -> DrawMode {
        self.draw_mode
    }

    /// 设置绘图模式
    pub fn set_draw_mode(&mut self, mode: DrawMode) {
        self.draw_mode = mode;
    }

    /// 获取当前温度
    pub const fn temperature(&self) -> i8 {
        self.temperature
    }

    /// 设置当前温度
    ///
    /// 温度影响波形选择，不同温度下墨水微粒响应速度不同
    pub fn set_temperature(&mut self, temperature: i8) {
        self.temperature = temperature;
    }

    /// 获取帧缓冲区引用
    pub fn frame_buffer(&self) -> &FrameBuffer {
        &self.frame_buffer
    }

    /// 获取帧缓冲区可变引用
    pub fn frame_buffer_mut(&mut self) -> &mut FrameBuffer {
        &mut self.frame_buffer
    }

    // ========================================================================
    // 绘图方法
    // ========================================================================

    /// 清空帧缓冲区为白色
    pub fn clear_white(&mut self) {
        self.frame_buffer.clear(GRAYSCALE_WHITE);
    }

    /// 清空帧缓冲区为黑色
    pub fn clear_black(&mut self) {
        self.frame_buffer.clear(GRAYSCALE_BLACK);
    }

    /// 清空帧缓冲区为指定灰度
    pub fn clear(&mut self, grayscale: u8) {
        self.frame_buffer.clear(grayscale);
    }

    /// 设置像素
    #[inline]
    pub fn set_pixel(&mut self, x: u32, y: u32, grayscale: u8) {
        self.frame_buffer.set_pixel(x, y, grayscale);
    }

    /// 获取像素
    #[inline]
    pub fn get_pixel(&self, x: u32, y: u32) -> u8 {
        self.frame_buffer.get_pixel(x, y)
    }

    /// 填充矩形
    pub fn fill_rect(&mut self, x: u32, y: u32, width: u32, height: u32, grayscale: u8) {
        self.frame_buffer.fill_rect(x, y, width, height, grayscale);
    }

    /// 绘制水平线
    pub fn draw_h_line(&mut self, x: u32, y: u32, length: u32, grayscale: u8) {
        self.frame_buffer.draw_h_line(x, y, length, grayscale);
    }

    /// 绘制垂直线
    pub fn draw_v_line(&mut self, x: u32, y: u32, length: u32, grayscale: u8) {
        self.frame_buffer.draw_v_line(x, y, length, grayscale);
    }

    /// 绘制矩形边框
    pub fn draw_rect(&mut self, x: u32, y: u32, width: u32, height: u32, grayscale: u8) {
        // 上边
        self.draw_h_line(x, y, width, grayscale);
        // 下边
        self.draw_h_line(x, y + height.saturating_sub(1), width, grayscale);
        // 左边
        self.draw_v_line(x, y, height, grayscale);
        // 右边
        self.draw_v_line(x + width.saturating_sub(1), y, height, grayscale);
    }

    // ========================================================================
    // 波形和刷新
    // ========================================================================

    /// 获取当前温度对应的波形相位数据
    pub fn get_waveform_phase(&self, mode: DrawMode) -> Option<&'static WaveformPhase> {
        let waveform_mode = self.waveform.get_mode(mode.mode_type())?;
        self.waveform.get_phases_for_temperature(waveform_mode, self.temperature)
    }

    /// 交换帧缓冲区
    ///
    /// 将当前帧保存为上一帧，用于下次差分刷新
    pub fn swap_buffers(&mut self) {
        core::mem::swap(&mut self.frame_buffer, &mut self.prev_buffer);
    }

    /// 复制当前帧到上一帧
    pub fn copy_to_prev(&mut self) {
        self.prev_buffer.data.copy_from_slice(&self.frame_buffer.data);
    }
}

impl Default for Ed047tc1 {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 显示驱动 Trait
// ============================================================================

/// 显示屏驱动 Trait
///
/// 定义了显示屏的基本操作接口，需要在具体平台上实现
pub trait DisplayDriver {
    /// 错误类型
    type Error;
    /// 初始化显示屏
    fn init(&mut self) -> Result<(), Self::Error>;
    /// 刷新全屏
    fn refresh(&mut self, mode: DrawMode) -> Result<(), Self::Error>;
    /// 局部刷新
    fn refresh_area(&mut self, x: u32, y: u32, width: u32, height: u32, mode: DrawMode) -> Result<(), Self::Error>;
    /// 清屏
    fn clear_screen(&mut self) -> Result<(), Self::Error>;
    /// 进入休眠模式
    fn sleep(&mut self) -> Result<(), Self::Error>;
    /// 唤醒
    fn wake(&mut self) -> Result<(), Self::Error>;
    /// 设置电源状态
    fn set_power(&mut self, enable: bool) -> Result<(), Self::Error>;
}

// ============================================================================
// 行数据构建
// ============================================================================

/// 行缓冲区大小（每行的字节数）
/// ED047TC1 宽度为 540 像素，每 4 个像素打包为 1 字节
pub const ROW_BUFFER_SIZE: usize = (WIDTH as usize + 3) / 4;

/// 行数据
///
/// 用于存储一行的显示数据，已转换为驱动格式
#[derive(Clone)]
pub struct RowData {
    /// 行数据缓冲区
    pub data: [u8; ROW_BUFFER_SIZE],
}

impl RowData {
    /// 创建空行数据
    pub const fn new() -> Self {
        Self {
            data: [0; ROW_BUFFER_SIZE],
        }
    }

    /// 从帧缓冲区构建行数据
    ///
    /// 根据波形 LUT 查找表，将源灰度和目标灰度转换为驱动信号
    ///
    /// # 参数
    /// * `frame_buffer` - 目标帧缓冲区
    /// * `prev_buffer` - 上一帧缓冲区（源灰度）
    /// * `row` - 行号
    /// * `phase` - 当前相位
    /// * `waveform_phase` - 波形相位数据
    pub fn build_from_buffers(
        &mut self,
        frame_buffer: &FrameBuffer,
        prev_buffer: &FrameBuffer,
        row: u32,
        phase: usize,
        waveform_phase: &WaveformPhase,
    ) {
        let lut = &waveform_phase.lut[phase];

        for x in (0..WIDTH).step_by(4) {
            let mut byte = 0u8;

            for i in 0..4 {
                let px = x + i;
                if px >= WIDTH {
                    break;
                }

                // 获取源灰度（上一帧）和目标灰度（当前帧）
                let source_gray = prev_buffer.get_pixel(px, row) as usize;
                let target_gray = frame_buffer.get_pixel(px, row) as usize;

                // 从 LUT 查找驱动信号
                // LUT 格式: lut[源灰度][4字节打包数据]
                // 每个字节包含 4 个目标灰度的驱动信号（每个 2 位）
                let lut_byte_idx = target_gray / 4;
                let lut_bit_shift = (3 - (target_gray % 4)) * 2;
                let drive_signal = (lut[source_gray][lut_byte_idx] >> lut_bit_shift) & 0x03;

                // 将驱动信号打包到输出字节
                byte |= drive_signal << ((3 - i) * 2);
            }

            let byte_index = (x / 4) as usize;
            self.data[byte_index] = byte;
        }
    }
}

impl Default for RowData {
    fn default() -> Self {
        Self::new()
    }
}
