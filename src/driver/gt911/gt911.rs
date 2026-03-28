use core::str;
use embedded_hal::i2c::I2c;

pub const GT911_I2C_ADDRESS_5D: u8 = 0x5D;
pub const GT911_I2C_ADDRESS_14: u8 = 0x14;

/// Command register
const COMMAND_REGISTER: u16 = 0x8040;

/// GT911产品ID
pub const PRODUCT_ID_REGISTER: u16 = 0x8140;
/// 触摸状态寄存器
pub const TOUCH_STATUS_REGISTER: u16 = 0x814E;
/// 第一个触摸点寄存器
pub const FIRST_TOUCH_POINT_REGISTER: u16 = 0x814F;

const TOUCH_POINT_ENTRY_LENGTH: usize = 8;
/// 最大触摸点
const MAXIMUM_TOUCH: usize = 5;

/// 触摸点
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Point {
    /// 触摸点数字（从0开始）
    pub id: u8,
    /// X坐标的屏幕像素
    pub x: u16,
    /// Y坐标的屏幕像素
    pub y: u16,
    /// 触点面积
    pub area: u16,
}

/// Gt911 异常
#[derive(Debug, Clone)]
pub enum Exception<E> {
    /// 未知产品ID
    UnknownProductId,
    /// I2C通信错误
    I2C(E),
    /// 没有新的数据可用，可以安全地忽略
    NotReady,
    /// 无效的I2C地址
    InvalidAddress,
    /// 无效的触摸数量
    InvalidTouchCount,
}

/// GT911
#[derive(Clone, Debug)]
pub struct GT911(u8);

impl GT911 {
    /// 使用指定的i2c地址创建一个新实例
    pub fn default() -> Self {
        GT911(GT911_I2C_ADDRESS_5D)
    }
    /// 使用指定的i2c地址创建一个新实例
    /// # Panic
    /// 如果地址不是有效的 GT911 地址 (0x5D 或 0x14)，则在 debug 模式下 panic
    pub fn new(address: u8) -> Self {
        debug_assert!(
            address == GT911_I2C_ADDRESS_5D || address == GT911_I2C_ADDRESS_14,
            "Invalid GT911 I2C address: 0x{:02X}, expected 0x5D or 0x14",
            address
        );
        GT911(address)
    }
    /// 检查ProductId的"911\0"字符串响应并重置状态寄存器，只需要在启动时调用一次
    /// # Error
    /// - 如果 I2C 地址无效，返回 `Exception::InvalidAddress`
    /// - 如果产品ID不匹配，返回 `Exception::UnknownProductId`
    /// - 如果 I2C 通信失败，返回 `Exception::I2C`
    pub fn init<I2C: I2c<Error = E>, E>(&self, i2c: &mut I2C) -> Result<(), Exception<E>> {
        // 验证地址有效性
        if self.0 != GT911_I2C_ADDRESS_5D && self.0 != GT911_I2C_ADDRESS_14 {
            return Err(Exception::InvalidAddress);
        }
        // switch to command mode
        self.write(i2c, COMMAND_REGISTER, 0)?;
        // read the product_id and confirm that it is expected
        let mut buffer: [u8; 4] = [0; 4];
        self.read(i2c, PRODUCT_ID_REGISTER, &mut buffer)?;
        match str::from_utf8(&buffer) {
            Ok(product_id) => {
                if product_id != "911\0" {
                    return Err(Exception::UnknownProductId);
                }
            }
            Err(_) => {
                return Err(Exception::UnknownProductId);
            }
        }
        // 清除状态寄存器
        self.write(i2c, TOUCH_STATUS_REGISTER, 0)?;
        Ok(())
    }

    /// 获取单个触摸点。
    /// 释放返回Ok(None)，按下或移动返回Some(point)，无数据返回Err（Exception::NotReady）。
    pub fn get_single_touch<I2C: I2c<Error = E>, E>(&self, i2c: &mut I2C) -> Result<Option<Point>, Exception<E>> {
        let touch_point_count: usize = self.get_touch_point_count(i2c)?;
        // 如果有触摸点，则读取第一个触摸点
        let point: Option<Point> = if touch_point_count > 0 {
            let mut buffer = [0; TOUCH_POINT_ENTRY_LENGTH];
            self.read(i2c, FIRST_TOUCH_POINT_REGISTER, &mut buffer)?;
            let point: Point = decode_point(&buffer);
            Some(point)
        } else {
            None
        };
        // 清除状态寄存器
        self.write(i2c, TOUCH_STATUS_REGISTER, 0)?;
        Ok(point)
    }

    /// Gets multiple stack allocated touch points (0-5 points)
    /// Returns points.len()==0 for release, points.len()>0 for press or move and Err(Error::NotReady) for no data
    pub fn get_multi_touch<I2C: I2c<Error = E>, E>(
        &self,
        i2c: &mut I2C,
    ) -> Result<[Point; MAXIMUM_TOUCH], Exception<E>> {
        let touch_point_count: usize = self.get_touch_point_count(i2c)?;

        let mut point_list: [Point; MAXIMUM_TOUCH] = [Point::default(); MAXIMUM_TOUCH];
        // 触摸点数在5个之间才处理
        if touch_point_count > 0 && touch_point_count <= MAXIMUM_TOUCH {
            // read touch point
            let mut buffer = [0u8; TOUCH_POINT_ENTRY_LENGTH * MAXIMUM_TOUCH];
            self.read(
                i2c,
                FIRST_TOUCH_POINT_REGISTER,
                &mut buffer[..TOUCH_POINT_ENTRY_LENGTH * MAXIMUM_TOUCH],
            )?;

            for n in 0..touch_point_count {
                let start = n * TOUCH_POINT_ENTRY_LENGTH;
                let point = decode_point(&buffer[start..start + TOUCH_POINT_ENTRY_LENGTH]);
                point_list[n] = point;
            }
        };

        // clear status register
        self.write(i2c, TOUCH_STATUS_REGISTER, 0)?;
        Ok(point_list)
    }

    fn get_touch_point_count<I2C: I2c<Error = E>, E>(&self, i2c: &mut I2C) -> Result<usize, Exception<E>> {
        // 读取触摸状态寄存器
        let mut buffer: [u8; 1] = [0; 1];
        self.read(i2c, TOUCH_STATUS_REGISTER, &mut buffer)?;

        let status: u8 = buffer[0];
        let ready: bool = (status & 0x80) > 0;
        let count: usize = (status & 0x0F) as usize;
        if ready { Ok(count) } else { Err(Exception::NotReady) }
    }

    fn write<I2C: I2c<Error = E>, E>(&self, i2c: &mut I2C, register: u16, value: u8) -> Result<(), Exception<E>> {
        let buffer = register.to_be_bytes();
        let command: [u8; 3] = [buffer[0], buffer[1], value];
        (&mut *i2c).write(self.0, &command).map_err(Exception::I2C)
    }

    fn read<I2C: I2c<Error = E>, E>(&self, i2c: &mut I2C, register: u16, buf: &mut [u8]) -> Result<(), Exception<E>> {
        (&mut *i2c)
            .write_read(self.0, &register.to_be_bytes(), buf)
            .map_err(Exception::I2C)
    }
}

fn decode_point(buffer: &[u8]) -> Point {
    assert!(buffer.len() >= TOUCH_POINT_ENTRY_LENGTH);
    Point {
        id: buffer[0],
        x: u16::from_le_bytes([buffer[1], buffer[2]]),
        y: u16::from_le_bytes([buffer[3], buffer[4]]),
        area: u16::from_le_bytes([buffer[5], buffer[6]]),
        // NOTE: the last byte is reserved
    }
}
