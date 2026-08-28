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

#[derive(Debug, Clone, Copy)]
pub struct Vec4 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct Mat4 {
    pub cols: [[f32; 4]; 4],
}

impl Mat4 {
    pub fn identity() -> Self {
        Self {
            cols: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    pub fn to_radians(degrees: f32) -> f32 {
        degrees * std::f32::consts::PI / 180.0
    }

    pub fn translation(tx: f32, ty: f32, tz: f32) -> Self {
        Self {
            cols: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [tx, ty, tz, 1.0],
            ],
        }
    }

    pub fn rotation_x(degrees: f32) -> Self {
        let angle = Self::to_radians(degrees);

        let sin = angle.sin();
        let cos = angle.cos();

        Self {
            cols: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, cos, sin, 0.0],
                [0.0, -sin, cos, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    pub fn rotation_y(degrees: f32) -> Self {
        let angle = Self::to_radians(degrees);

        let sin = angle.sin();
        let cos = angle.cos();

        Self {
            cols: [
                [cos, 0.0, -sin, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [sin, 0.0, cos, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    pub fn rotation_z(degrees: f32) -> Self {
        let angle = Self::to_radians(degrees);

        let sin = angle.sin();
        let cos = angle.cos();

        Self {
            cols: [
                [cos, sin, 0.0, 0.0],
                [-sin, cos, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    pub fn scaling(sx: f32, sy: f32, sz: f32) -> Self {
        Self {
            cols: [
                [sx, 0.0, 0.0, 0.0],
                [0.0, sy, 0.0, 0.0],
                [0.0, 0.0, sz, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    pub fn look_at(eye: Vec3, target: Vec3, up: Vec3) -> Self {
        let z_axis = (eye - target).normalize();
        let x_axis = up.cross(&z_axis).normalize();
        let y_axis = z_axis.cross(&x_axis);

        Self {
            cols: [
                [x_axis.x, y_axis.x, z_axis.x, 0.0],
                [x_axis.y, y_axis.y, z_axis.y, 0.0],
                [x_axis.z, y_axis.z, z_axis.z, 0.0],
                [-x_axis.dot(&eye), -y_axis.dot(&eye), -z_axis.dot(&eye), 1.0],
            ],
        }
    }

    pub fn perspective(fov_degrees: f32, aspect_ratio: f32, near: f32, far: f32) -> Self {
        let fov_radians = fov_degrees * std::f32::consts::PI / 180.0;

        let tan = (fov_radians / 2.0).tan();
        let scale_y = 1.0 / tan;
        let scale_x = scale_y / aspect_ratio;

        // Корректируем коэффициенты под отрицательную ось Z (стандарт OpenGL/LookAt)
        let remap_z = -(far + near) / (far - near);
        let remap_w = -(2.0 * far * near) / (far - near);

        Self {
            cols: [
                [scale_x, 0.0, 0.0, 0.0],
                [0.0, scale_y, 0.0, 0.0],
                [0.0, 0.0, remap_z, -1.0], // Поставили -1.0, чтобы res_w стал положительным!
                [0.0, 0.0, remap_w, 0.0],
            ],
        }
    }

    /// Преобразование вектора-НАПРАВЛЕНИЯ (нормали, оси, скорости).
    ///
    /// Отличие от `&Mat4 * Vec3`: там подставляется `w = 1.0`, то есть вектор
    /// считается точкой и к нему прибавляется столбец трансляции `cols[3]`.
    /// Направление сдвигать нельзя — оно задаёт ориентацию, а не место,
    /// поэтому здесь `w = 0.0` и трансляция просто не участвует.
    ///
    /// Верно для поворотов и равномерного масштаба (после `.normalize()`).
    /// Для неравномерного масштаба нормали требуют обратно-транспонированной
    /// матрицы — этого здесь пока нет.
    pub fn transform_dir(&self, dir: Vec3) -> Vec3 {
        let c = self.cols;

        Vec3::new(
            c[0][0] * dir.x + c[1][0] * dir.y + c[2][0] * dir.z,
            c[0][1] * dir.x + c[1][1] * dir.y + c[2][1] * dir.z,
            c[0][2] * dir.x + c[1][2] * dir.y + c[2][2] * dir.z,
        )
    }
}

impl std::ops::Mul<Vec3> for &Mat4 {
    type Output = Vec4;

    fn mul(self, vec: Vec3) -> Vec4 {
        let c = self.cols;

        let w = 1.0;

        let res_x = c[0][0] * vec.x + c[1][0] * vec.y + c[2][0] * vec.z + c[3][0] * w;
        let res_y = c[0][1] * vec.x + c[1][1] * vec.y + c[2][1] * vec.z + c[3][1] * w;
        let res_z = c[0][2] * vec.x + c[1][2] * vec.y + c[2][2] * vec.z + c[3][2] * w;
        let res_w = c[0][3] * vec.x + c[1][3] * vec.y + c[2][3] * vec.z + c[3][3] * w;

        Vec4 {
            x: res_x,
            y: res_y,
            z: res_z,
            w: res_w,
        }
    }
}

impl std::ops::Mul<&Mat4> for &Mat4 {
    type Output = Mat4;

    fn mul(self, rhs: &Mat4) -> Mat4 {
        let mut result_cols = [[0.0; 4]; 4];

        for col in 0..4 {
            for row in 0..4 {
                result_cols[col][row] = self.cols[0][row] * rhs.cols[col][0]
                    + self.cols[1][row] * rhs.cols[col][1]
                    + self.cols[2][row] * rhs.cols[col][2]
                    + self.cols[3][row] * rhs.cols[col][3];
            }
        }

        Mat4 { cols: result_cols }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Свой допуск, а не config::EPSILON: тот задаёт геометрический порог
    /// в клиппинге, а здесь речь про накопленную ошибку f32
    const EPS: f32 = 1e-5;

    fn assert_vec3_eq(actual: Vec3, expected: Vec3) {
        let d = actual - expected;

        assert!(
            d.length() < EPS,
            "ожидали {:?}, получили {:?}",
            expected,
            actual
        );
    }

    // --- transform_dir: направление против точки ---

    #[test]
    fn transform_dir_ignores_translation() {
        let model = Mat4::translation(10.0, -5.0, 3.0);
        let dir = Vec3::new(0.0, 0.0, 1.0);

        assert_vec3_eq(model.transform_dir(dir), dir);
    }

    #[test]
    fn mul_vec3_applies_translation() {
        // Обратная сторона: точка трансляцию получить ОБЯЗАНА.
        // Если этот тест сломается — сломан весь пайплайн, а не только свет
        let model = Mat4::translation(10.0, -5.0, 3.0);
        let point = &model * Vec3::new(0.0, 0.0, 1.0);

        assert_vec3_eq(Vec3::new(point.x, point.y, point.z), Vec3::new(10.0, -5.0, 4.0));
    }

    #[test]
    fn transform_dir_applies_rotation() {
        // Поворот на 90° вокруг Y переводит +Z в +X
        let model = Mat4::rotation_y(90.0);

        assert_vec3_eq(
            model.transform_dir(Vec3::new(0.0, 0.0, 1.0)),
            Vec3::new(1.0, 0.0, 0.0),
        );
    }

    #[test]
    fn normal_of_distant_instance_keeps_direction() {
        // Регрессия: куб из кольца — далеко от начала координат и уменьшен.
        // Пока нормаль умножалась как точка, к ней прибавлялась позиция (6, -2, 0)
        // и «нормаль» превращалась в направление на объект — свет врал
        let model = &Mat4::translation(6.0, -2.0, 0.0) * &Mat4::scaling(0.3, 0.3, 0.3);
        let n_local = Vec3::new(0.0, 1.0, 0.0);

        // Равномерный масштаб меняет длину, но не направление — normalize() его убирает
        assert_vec3_eq(model.transform_dir(n_local).normalize(), n_local);
    }

    // --- матрицы камеры и проекции ---

    #[test]
    fn look_at_puts_eye_at_origin() {
        let eye = Vec3::new(3.0, 4.0, 5.0);
        let view = Mat4::look_at(eye, Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));

        // Смысл матрицы вида: перенести мир так, чтобы камера села в начало координат
        let p = &view * eye;

        assert_vec3_eq(Vec3::new(p.x, p.y, p.z), Vec3::new(0.0, 0.0, 0.0));
    }

    #[test]
    fn perspective_w_is_distance_in_front_of_camera() {
        let proj = Mat4::perspective(75.0, 4.0 / 3.0, 0.1, 100.0);

        // Камера смотрит вдоль -Z, значит точка в 5 единицах перед ней — это z = -5.
        // Именно на этом держится depth-буфер: он хранит 1/w
        let clip = &proj * Vec3::new(0.0, 0.0, -5.0);

        assert!((clip.w - 5.0).abs() < EPS, "w = {}", clip.w);
    }

    #[test]
    fn perspective_maps_near_and_far_to_ndc_range() {
        let (near, far) = (0.1, 100.0);
        let proj = Mat4::perspective(75.0, 4.0 / 3.0, near, far);

        let at_near = &proj * Vec3::new(0.0, 0.0, -near);
        let at_far = &proj * Vec3::new(0.0, 0.0, -far);

        // После перспективного деления ближняя плоскость даёт -1, дальняя +1
        assert!((at_near.z / at_near.w + 1.0).abs() < EPS);
        assert!((at_far.z / at_far.w - 1.0).abs() < EPS);
    }

    // --- базовая алгебра ---

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

    #[test]
    fn identity_is_neutral_for_multiplication() {
        let m = &Mat4::translation(1.0, 2.0, 3.0) * &Mat4::rotation_z(30.0);
        let same = &m * &Mat4::identity();

        for col in 0..4 {
            for row in 0..4 {
                assert!((m.cols[col][row] - same.cols[col][row]).abs() < EPS);
            }
        }
    }
}
