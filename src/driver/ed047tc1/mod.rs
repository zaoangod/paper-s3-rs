//! ED047TC1 4.7 英寸 16 级灰度电子墨水屏驱动
//!
//! ED047TC1 是一款 4.7 英寸电子墨水显示屏，支持 16 级灰度（4-bit）。
//! 分辨率为 540 x 960 像素。
//!
//! # 特性
//!
//! - 16 级灰度 (0-15)
//! - 多种刷新模式：DU（快速）、GC16（全刷新）、GL16（无闪烁）
//! - 支持局部刷新
//! - 温度自适应波形
//!
//! # 刷新模式
//!
//! | 模式          | 名称         | 相位数 | 用途                   |
//! |---------------|--------------|--------|------------------------|
//! | DU            | 直接更新     | 5      | 快速黑白刷新，适合文字 |
//! | GC16          | 全灰度闪烁   | 30     | 高质量灰度显示，会闪烁 |
//! | GL16          | 全灰度无闪烁 | 30     | 平滑灰度过渡，无闪烁   |
//! | WHITE_TO_GL16 | 白底到灰度   | 15     | 从白色背景快速过渡     |
//! | BLACK_TO_GL16 | 黑底到灰度   | 15     | 从黑色背景快速过渡     |

pub mod waveform;

pub use self::waveform::{ED047TC1, Waveform, WaveformMode, WaveformPhase};

/// 显示屏宽度（像素）
pub const WIDTH: u32 = 540;
/// 显示屏高度（像素）
pub const HEIGHT: u32 = 960;

// ============================================================================
// 绘图模式
// ============================================================================

/// 绘图模式
///
/// 不同模式适用于不同场景，在刷新速度和显示质量之间权衡。
#[derive(Eq, Copy, Clone, Debug, Default, PartialEq)]
pub enum DrawMode {
    /// 直接更新（DU）- 快速黑白刷新
    ///
    /// - 5 个相位
    /// - 最快的刷新速度
    /// - 仅适合黑白内容
    /// - 适合文字显示
    Du,

    /// 全灰度带闪烁（GC16）
    ///
    /// - 30 个相位
    /// - 最高显示质量
    /// - 刷新时会闪烁
    /// - 适合图片显示
    #[default]
    Gc16,

    /// 全灰度无闪烁（GL16）
    ///
    /// - 30 个相位
    /// - 无闪烁刷新
    /// - 适合灰度渐变显示
    Gl16,

    /// 白底到灰度
    ///
    /// - 15 个相位
    /// - 适用于已知白色背景的情况
    /// - 比 GC16 快一倍
    WhiteToGl16,

    /// 黑底到灰度
    ///
    /// - 15 个相位
    /// - 适用于已知黑色背景的情况
    /// - 比 GC16 快一倍
    BlackToGl16,
}

impl DrawMode {
    /// 获取对应的波形模式类型 ID
    pub const fn mode_type(&self) -> u8 {
        match self {
            DrawMode::Du => 1,
            DrawMode::Gc16 => 2,
            DrawMode::Gl16 => 5,
            DrawMode::WhiteToGl16 => 16,
            DrawMode::BlackToGl16 => 17,
        }
    }

    /// 获取该模式的相位数量
    pub const fn phase_count(&self) -> u8 {
        match self {
            DrawMode::Du => 5,
            DrawMode::Gc16 => 30,
            DrawMode::Gl16 => 30,
            DrawMode::WhiteToGl16 => 15,
            DrawMode::BlackToGl16 => 15,
        }
    }
}

// ============================================================================
// 计算缓冲区大小
// ============================================================================

/// 计算 4-bit 灰度缓冲区所需的字节数
///
/// 每个像素占用 4 位，因此每字节存储 2 个像素
#[inline]
pub const fn buffer_len_4bpp(width: usize, height: usize) -> usize {
    (width * height + 1) / 2
}

/// ED047TC1 的缓冲区大小
pub const BUFFER_SIZE: usize = buffer_len_4bpp(WIDTH as usize, HEIGHT as usize);
