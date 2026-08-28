use crate::math::Vec3;

//Настройки окна

/// Ширина окна (пикс)
pub const WIDTH: u32 = 800;
/// Высота окна (пикс)
pub const HEIGHT: u32 = 600;
/// Заголовок окна
pub const WINDOW_TITLE: &str = "Rust 3D Engine";

// Настройки фона и цвета ребер

/// Цвет фона (4 байт)
pub const CLEAR_COLOR: [u8; 4] = [20, 20, 20, 255];
/// Цвет ребра (4 байт)
pub const LINE_COLOR: [u8; 4] = [0, 255, 0, 255];

// Освещение

/// Направление НА источник света (солнце: лучи параллельны, позиции нет)
pub const LIGHT_DIRECTION: Vec3 = Vec3::new(0.5, 1.0, 0.8);
/// Фоновая подсветка, чтобы неосвещённая сторона не была чёрной
pub const AMBIENT_LIGHT: f32 = 0.25;

// Настройки камеры

/// Угол обзора (град)
pub const DEFAULT_FOV: f32 = 75.0;
/// Ближняя плоскость
pub const DEFAULT_NEAR: f32 = 0.1;
/// Дальняя плоскость
pub const DEFAULT_FAR: f32 = 100.0;
/// Скорость передвижения камеры
pub const CAMERA_MOVEMENT_SPEED: f32 = 4.0;
/// Скорость поворотов камеры
pub const CAMERA_ROTATION_SPEED: f32 = 100.0;

// Физика

/// Число около нуля для проверок
pub const EPSILON: f32 = 1e-5;
