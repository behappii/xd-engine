use crate::{
    config::{DEFAULT_FAR, DEFAULT_FOV, DEFAULT_NEAR, HEIGHT, WIDTH},
    math::{Mat4, Vec3, Vec4},
    renderer::draw_triangle_wireframe,
};

#[derive(Clone, Debug)]
pub struct Mesh {
    pub vertices: Vec<Vec3>,
    pub triangles: Vec<[usize; 3]>,
}

impl Mesh {
    pub fn create_cube() -> Self {
        Self {
            vertices: vec![
                Vec3::new(-1.0, -1.0, -1.0), // 0
                Vec3::new(1.0, -1.0, -1.0),  // 1
                Vec3::new(1.0, 1.0, -1.0),   // 2
                Vec3::new(-1.0, 1.0, -1.0),  // 3
                Vec3::new(-1.0, -1.0, 1.0),  // 4
                Vec3::new(1.0, -1.0, 1.0),   // 5
                Vec3::new(1.0, 1.0, 1.0),    // 6
                Vec3::new(-1.0, 1.0, 1.0),   // 7
            ],
            triangles: vec![
                // Back (-Z)
                [0, 1, 2],
                [0, 2, 3],
                // Front (+Z)
                [4, 6, 5],
                [4, 7, 6],
                // Bottom (-Y)
                [0, 5, 1],
                [0, 4, 5],
                // Top (+Y)
                [3, 2, 6],
                [3, 6, 7],
                // Left (-X)
                [0, 3, 7],
                [0, 7, 4],
                // Right (+X)
                [1, 5, 6],
                [1, 6, 2],
            ],
        }
    }
    pub fn create_pyramid() -> Self {
        Self {
            vertices: vec![
                Vec3::new(-1.0, -1.0, -1.0), // 0
                Vec3::new(-1.0, -1.0, 1.0),  // 1
                Vec3::new(1.0, -1.0, -1.0),  // 2
                Vec3::new(1.0, -1.0, 1.0),   // 3
                Vec3::new(0.0, 1.0, 0.0),    // 4
            ],

            triangles: vec![
                // Основание
                [0, 2, 3],
                [0, 3, 1],
                // Боковые грани
                [0, 4, 2],
                [2, 4, 3],
                [3, 4, 1],
                [1, 4, 0],
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
    pub camera_position: Vec3,
    pub yaw: f32,   // поворот камеры влево/вправо в градусах
    pub pitch: f32, // камера вверх/вниз в градусах
}

impl Scene {
    // Создание сцены
    pub fn new() -> Self {
        Self {
            instances: Vec::new(),
            camera_position: Vec3::new(0.0, 0.0, 5.0),
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

        // Расчет матрицы Камеры (Она едина для всей сцены)
        let target_pos = self.camera_position + forward;
        let up_vector = Vec3::new(0.0, 1.0, 0.0);
        let view_matrix = Mat4::look_at(self.camera_position, target_pos, up_vector);

        // Настройки матрицы перспективы
        let aspect = WIDTH as f32 / HEIGHT as f32;

        // Сам рассчет матрицы
        let projection_matrix = Mat4::perspective(DEFAULT_FOV, aspect, DEFAULT_NEAR, DEFAULT_FAR);

        // Объединяем View * Projection один раз для кадра
        let vp_matrix = &projection_matrix * &view_matrix;

        // Рендеринг каждого инстанса сцены
        for instance in &self.instances {
            let model_matrix = instance.get_model_matrix();

            let mvp_matrix = &vp_matrix * &model_matrix;

            let mut clip_vertices = vec![
                Vec4 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    w: 1.0
                };
                instance.mesh.vertices.len()
            ];

            // Проецируем вершины конкретного меша
            for (i, &vertex) in instance.mesh.vertices.iter().enumerate() {
                clip_vertices[i] = &mvp_matrix * vertex;
            }

            // Отрисовываем грани этого меша с отсечением невидимых
            for triangle in &instance.mesh.triangles {
                let v0 = clip_vertices[triangle[0]];
                let v1 = clip_vertices[triangle[1]];
                let v2 = clip_vertices[triangle[2]];

                draw_triangle_wireframe(v0, v1, v2, frame);
            }
        }
    }
}
