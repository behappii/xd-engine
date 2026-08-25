use crate::{
    math::{Mat4, Vec3},
    renderer::{HEIGHT, WIDTH, draw_line, project},
};

#[derive(Clone, Debug)]
pub struct Mesh {
    pub vertices: Vec<Vec3>,
    pub faces: Vec<[usize; 4]>,
}

impl Mesh {
    pub fn create_cube() -> Self {
        Self {
            vertices: vec![
                Vec3::new(-1.0, -1.0, -1.0),
                Vec3::new(1.0, -1.0, -1.0),
                Vec3::new(1.0, 1.0, -1.0),
                Vec3::new(-1.0, 1.0, -1.0),
                Vec3::new(-1.0, -1.0, 1.0),
                Vec3::new(1.0, -1.0, 1.0),
                Vec3::new(1.0, 1.0, 1.0),
                Vec3::new(-1.0, 1.0, 1.0),
            ],
            faces: vec![
                [0, 3, 2, 1],
                [4, 5, 6, 7],
                [0, 1, 5, 4],
                [3, 7, 6, 2],
                [0, 4, 7, 3],
                [1, 2, 6, 5],
            ],
        }
    }
    pub fn create_pyramid() -> Self {
        Self {
            vertices: vec![
                Vec3::new(-1.0, -1.0, -1.0),
                Vec3::new(-1.0, -1.0, 1.0),
                Vec3::new(1.0, -1.0, -1.0),
                Vec3::new(1.0, -1.0, 1.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            faces: vec![
                [0, 2, 3, 1],
                [0, 4, 2, 4],
                [2, 4, 3, 4],
                [3, 4, 1, 4],
                [1, 4, 0, 4],
            ],
        }
    }
}

pub struct Instance {
    pub mesh: Mesh,
    pub position: Vec3,
    pub rotation: Vec3,
    pub scale: Vec3,
}

impl Instance {
    pub fn new(mesh: Mesh, position: Vec3) -> Self {
        Self {
            mesh,
            position,
            rotation: Vec3::new(0.0, 0.0, 0.0),
            scale: Vec3::new(1.0, 1.0, 1.0),
        }
    }

    pub fn get_model_matrix(&self) -> Mat4 {
        let scale_mat = Mat4::scaling(self.scale.x, self.scale.y, self.scale.z);
        let rot_x = Mat4::rotation_x(self.rotation.x);
        let rot_y = Mat4::rotation_y(self.rotation.y);
        let rot_z = Mat4::rotation_z(self.rotation.z);
        let trans_mat = Mat4::translation(self.position.x, self.position.y, self.position.z);

        let rotation = &rot_x * &(&rot_y * &rot_z);
        let model = &trans_mat * &(&rotation * &scale_mat);
        model
    }
}

pub struct Scene {
    // массив инстансов
    pub instances: Vec<Instance>,
    // камера
    camera_position: Vec3,
    yaw: f32,   // поворот камеры влево/вправо в градусах
    pitch: f32, // камера вверх/вниз в градусах
}

impl Scene {
    // Создание сцены
    pub fn new() -> Self {
        Self {
            instances: Vec::new(),
            camera_position: Vec3::new(0.0, 2.0, 6.0),
            yaw: -90.0,
            pitch: 0.0,
        }
    }

    pub fn add_instance(&mut self, instance: Instance) {
        self.instances.push(instance);
    }

    pub fn draw(&self, frame: &mut [u8]) {
        // Рассчитываем текущие векторы направления камеры
        let yaw_rad = self.yaw.to_radians();
        let pitch_rad = self.pitch.to_radians();

        let forward = Vec3::new(
            yaw_rad.cos() * pitch_rad.cos(),
            pitch_rad.sin(),
            yaw_rad.sin() * pitch_rad.cos(),
        )
        .normalize();

        let right = forward.cross(&Vec3::new(0.0, 1.0, 0.0)).normalize();

        // Расчет матрицы Камеры (Она едина для всей сцены)
        let target_pos = self.camera_position + forward;
        let up_vector = Vec3::new(0.0, 1.0, 0.0);
        let view_matrix = Mat4::look_at(self.camera_position, target_pos, up_vector);

        // Настройки матрицы перспективы
        let fov = 60.0;
        let aspect = WIDTH as f32 / HEIGHT as f32;
        let near = 0.1;
        let far = 100.0;
        // Сам рассчет матрицы
        let projection_matrix = Mat4::perspective(fov, aspect, near, far);

        // Объединяем View * Projection один раз для кадра
        let vp_matrix = &projection_matrix * &view_matrix;

        // Рендеринг каждого инстанса сцены
        for instance in &self.instances {
            let model_matrix = instance.get_model_matrix();

            let mvp_matrix = &vp_matrix * &model_matrix;

            // Проецируем вершины конкретного меша
            let mut projected = vec![(0, 0); instance.mesh.vertices.len()];
            for (i, &vertex) in instance.mesh.vertices.iter().enumerate() {
                let transformed = &mvp_matrix * vertex;
                projected[i] = project(transformed, WIDTH, HEIGHT);
            }

            // Отрисовываем грани этого меша с отсечением невидимых
            for face in &instance.mesh.faces {
                let p0 = projected[face[0]];
                let p1 = projected[face[1]];
                let p2 = projected[face[2]];

                let v1_x = (p1.0 - p0.0) as f32;
                let v1_y = (p1.1 - p0.1) as f32;
                let v2_x = (p2.0 - p0.0) as f32;
                let v2_y = (p2.1 - p0.1) as f32;

                let cross_z = v1_x * v2_y - v1_y * v2_x;

                // Наше исправленное условие отсечения
                if cross_z < 0.0 {
                    draw_line(p0.0, p0.1, p1.0, p1.1, frame);
                    draw_line(p1.0, p1.1, p2.0, p2.1, frame);
                    let p3 = projected[face[3]];
                    draw_line(p2.0, p2.1, p3.0, p3.1, frame);
                    draw_line(p3.0, p3.1, p0.0, p0.1, frame);
                }
            }
        }
    }
}
