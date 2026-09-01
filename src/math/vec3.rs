#[derive(Debug, Clone, Copy)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    // Создание нового вектора
    // const — чтобы вектор можно было записать в константу в config.rs
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    // Скалярное произведение векторов
    pub fn dot(&self, other: &Vec3) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    // Длина вектора
    pub fn length(&self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    // Делает длину вектора равной 1.0, сохраняя направление
    pub fn normalize(&self) -> Self {
        let len = self.length();

        if len == 0.0 {
            *self
        } else {
            Self::new(self.x / len, self.y / len, self.z / len)
        }
    }

    // Векторное произведение
    pub fn cross(&self, other: &Vec3) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }
}

impl std::ops::Add for Vec3 {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Vec3::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl std::ops::Mul<f32> for Vec3 {
    type Output = Self;
    fn mul(self, num: f32) -> Self {
        Vec3::new(self.x * num, self.y * num, self.z * num)
    }
}

impl std::ops::Sub for Vec3 {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Vec3::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}

/// Покомпонентное произведение (произведение Адамара).
///
/// В линейной алгебре у двух векторов «умножения» в таком смысле нет — там
/// dot и cross. Но у нас Vec3 подрабатывает ещё и цветом, а модуляция цвета
/// цветом — это ровно покомпонентное умножение: белый свет × красная текстура
/// = красный. Именно так свет умножается на текселя в `draw_triangle_filled`
impl std::ops::Mul<Vec3> for Vec3 {
    type Output = Self;
    fn mul(self, other: Self) -> Self {
        Vec3::new(self.x * other.x, self.y * other.y, self.z * other.z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::testing::assert_vec3_eq;

    #[test]
    fn cross_follows_right_hand_rule() {
        let x = Vec3::new(1.0, 0.0, 0.0);
        let y = Vec3::new(0.0, 1.0, 0.0);

        assert_vec3_eq(x.cross(&y), Vec3::new(0.0, 0.0, 1.0));
    }

    #[test]
    fn normalize_of_zero_vector_does_not_produce_nan() {
        let zero = Vec3::new(0.0, 0.0, 0.0).normalize();

        assert!(zero.x.is_finite() && zero.y.is_finite() && zero.z.is_finite());
    }
}
