/// Двумерный вектор. Заведён ради UV-координат: текстурная координата — такой
/// же интерполируемый атрибут вершины, как цвет, и ей нужны те же операции
/// (сложение, вычитание, умножение на число), чтобы ездить через `lerp` и
/// барицентрики
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Начало координат — «текстуры нет». Меши без развёртки заполняются им
    pub const ZERO: Self = Self::new(0.0, 0.0);

    /// Длина вектора.
    ///
    /// Понадобилась для мип-уровней: там UV перестаёт быть просто координатой
    /// и становится вектором сдвига по текстуре, а длина этого сдвига — размер
    /// отпечатка пикселя в текселях
    pub fn length(&self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
}

impl std::ops::Add for Vec2 {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Vec2::new(self.x + other.x, self.y + other.y)
    }
}

impl std::ops::Sub for Vec2 {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Vec2::new(self.x - other.x, self.y - other.y)
    }
}

impl std::ops::Mul<f32> for Vec2 {
    type Output = Self;
    fn mul(self, num: f32) -> Self {
        Vec2::new(self.x * num, self.y * num)
    }
}
