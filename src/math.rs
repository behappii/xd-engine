#[derive(Debug, Clone, Copy)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    // Создание нового вектора
    pub fn new(x: f32, y: f32, z: f32) -> Self {
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
