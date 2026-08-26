use crate::{
    math::{Mat4, Vec3, Vec4},
    renderer::{HEIGHT, WIDTH, draw_line},
};

// число около нуля для проверок if/else
pub const EPSILON: f32 = 1e-5;

#[derive(Clone, Copy)]
pub enum Plane {
    Left,
    Right,
    Bottom,
    Top,
    Near,
    Far,
}

// Расстояние до плоскости
pub fn plane_distance(v: &Vec4, plane: Plane) -> f32 {
    match plane {
        Plane::Left => v.x + v.w,
        Plane::Right => v.w - v.x,

        Plane::Bottom => v.y + v.w,
        Plane::Top => v.w - v.y,

        Plane::Near => v.z + v.w,
        Plane::Far => v.w - v.z,
    }
}

// Интерполяция вершины
pub fn lerp_vec4(a: Vec4, b: Vec4, t: f32) -> Vec4 {
    Vec4 {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
        z: a.z + (b.z - a.z) * t,
        w: a.w + (b.w - a.w) * t,
    }
}

// Клиппинг одной линии
pub fn clip_line_4d(mut v0: Vec4, mut v1: Vec4) -> Option<(Vec4, Vec4)> {
    let planes = [
        Plane::Left,
        Plane::Right,
        Plane::Bottom,
        Plane::Top,
        Plane::Near,
        Plane::Far,
    ];

    for plane in planes {
        let d0 = plane_distance(&v0, plane);
        let d1 = plane_distance(&v1, plane);

        let inside0 = d0 >= -EPSILON;
        let inside1 = d1 >= -EPSILON;

        match (inside0, inside1) {
            (true, true) => {}

            (false, false) => {
                return None;
            }

            _ => {
                let denom = d0 - d1;

                if denom.abs() < EPSILON {
                    return None;
                }

                let t = d0 / denom;

                let intersection = lerp_vec4(v0, v1, t);

                if inside0 {
                    v1 = intersection;
                } else {
                    v0 = intersection;
                }
            }
        }
    }

    Some((v0, v1))
}

// Perspective Divide
pub fn clip_to_ndc(v: Vec4) -> Vec3 {
    Vec3 {
        x: v.x / v.w,
        y: v.y / v.w,
        z: v.z / v.w,
    }
}

// Viewport Transform
pub fn ndc_to_screen(v: Vec3, width: u32, height: u32) -> (i32, i32) {
    let x = ((v.x + 1.0) * 0.5 * width as f32).round() as i32;

    let y = ((1.0 - v.y) * 0.5 * height as f32).round() as i32;

    (x, y)
}

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
    pub camera_position: Vec3,
    pub yaw: f32,   // поворот камеры влево/вправо в градусах
    pub pitch: f32, // камера вверх/вниз в градусах
}

impl Scene {
    // Создание сцены
    pub fn new() -> Self {
        Self {
            instances: Vec::new(),
            camera_position: Vec3::new(0.0, 3.0, 0.0),
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
        let fov = 75.0;
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
            for face in &instance.mesh.faces {
                let v0_4d = clip_vertices[face[0]];
                let v1_4d = clip_vertices[face[1]];
                let v2_4d = clip_vertices[face[2]];
                let v3_4d = clip_vertices[face[3]];

                let line_pairs = [
                    (v0_4d, v1_4d),
                    (v1_4d, v2_4d),
                    (v2_4d, v3_4d),
                    (v3_4d, v0_4d),
                ];

                for &(start, end) in &line_pairs {
                    // Прогоняем каждое ребро через наш 4D-клиппинг!
                    if let Some((p_start, p_end)) = clip_line_4d(start, end) {
                        // Рисуем только выжившую часть линии, идеально обрезанную плоскостями!
                        let ndc0 = clip_to_ndc(p_start);
                        let ndc1 = clip_to_ndc(p_end);

                        let p0 = ndc_to_screen(ndc0, WIDTH, HEIGHT);

                        let p1 = ndc_to_screen(ndc1, WIDTH, HEIGHT);

                        draw_line(p0.0, p0.1, p1.0, p1.1, frame);
                    }
                }
            }
        }
    }
}
