use std::rc::Rc;

use crate::{
    clipping::clip_triangle_near,
    config::{DEFAULT_FAR, DEFAULT_FOV, DEFAULT_NEAR, LINE_COLOR},
    math::{Mat4, Vec3, Vec4},
    renderer::{DrawContext, draw_triangle_filled, draw_triangle_wireframe, is_backface},
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
                [0, 2, 1],
                [0, 3, 2],
                // Front (+Z)
                [4, 5, 6],
                [4, 6, 7],
                // Bottom (-Y)
                [0, 1, 5],
                [0, 5, 4],
                // Top (+Y)
                [3, 6, 2],
                [3, 7, 6],
                // Left (-X)
                [0, 7, 3],
                [0, 4, 7],
                // Right (+X)
                [1, 6, 5],
                [1, 2, 6],
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
    pub mesh: Rc<Mesh>,
    pub position: Vec3,
    pub rotation: Vec3,
    pub scale: Vec3,

    // цвет всего объекта
    pub color: [u8; 4], // R G B A
    // необязательная раскраска по треугольникам
    // индекс совпадает с mesh.triangles
    pub face_colors: Option<Vec<[u8; 4]>>,

    pub wireframe: bool,
}

impl Instance {
    pub fn new(mesh: impl Into<Rc<Mesh>>, position: Vec3) -> Self {
        Self {
            mesh: mesh.into(),
            position,
            rotation: Vec3::new(0.0, 0.0, 0.0),
            scale: Vec3::new(1.0, 1.0, 1.0),
            color: LINE_COLOR,
            face_colors: None,
            wireframe: false,
        }
    }
    /// Задать цвет всему инстансу
    pub fn with_color(mut self, color: [u8; 4]) -> Self {
        self.color = color;
        self
    }
    /// Раскраска по индексу треугольников
    pub fn with_face_colors(mut self, colors: Vec<[u8; 4]>) -> Self {
        self.face_colors = Some(colors);
        self
    }

    pub fn as_wireframe(mut self) -> Self {
        self.wireframe = true;
        self
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

    pub fn draw(&self, frame: &mut [u8], depth: &mut [f32], width: u32, height: u32) {
        // Рассчитываем текущие векторы направления камеры
        let yaw_rad = self.yaw.to_radians();
        let pitch_rad = self.pitch.to_radians();

        let forward = Vec3::new(
            yaw_rad.cos() * pitch_rad.cos(),
            pitch_rad.sin(),
            yaw_rad.sin() * pitch_rad.cos(),
        )
        .normalize();

        // Расчет матрицы Вида (Она едина для всей сцены)
        let target_pos = self.camera_position + forward;
        let up_vector = Vec3::new(0.0, 1.0, 0.0);
        let view_matrix = Mat4::look_at(self.camera_position, target_pos, up_vector);

        // отношение ширина:высота экрана
        let aspect = width as f32 / height as f32;

        // Рассчет матрицы проекции на экран
        let projection_matrix = Mat4::perspective(DEFAULT_FOV, aspect, DEFAULT_NEAR, DEFAULT_FAR);

        // Объединяем View * Projection один раз для кадра
        let vp_matrix = &projection_matrix * &view_matrix;

        let mut ctx = DrawContext::new(frame, depth, width, height, LINE_COLOR);

        let mut clip_vertices: Vec<Vec4> = Vec::new();

        // Рендеринг каждого инстанса сцены
        for instance in &self.instances {
            let model_matrix = instance.get_model_matrix();
            let mvp_matrix = &vp_matrix * &model_matrix;

            clip_vertices.clear();
            clip_vertices.extend(
                instance
                    .mesh
                    .vertices
                    .iter()
                    .map(|&vertex| &mvp_matrix * vertex),
            );

            // яркость от наклона граней
            let light_dir = Vec3::new(0.5, 1.0, 0.8).normalize();

            // Отрисовываем грани этого меша с отсечением невидимых
            for (i, triangle) in instance.mesh.triangles.iter().enumerate() {
                let v0 = clip_vertices[triangle[0]];
                let v1 = clip_vertices[triangle[1]];
                let v2 = clip_vertices[triangle[2]];

                if is_backface(v0, v1, v2) {
                    continue; // грань отвернута - пропускаем
                }

                // цвет грани до освещения
                // Если раскраски по граням нет (или она короче) — берём цвет объекта
                let base_color = instance
                    .face_colors
                    .as_ref()
                    .and_then(|fc| fc.get(i))
                    .copied()
                    .unwrap_or(instance.color);

                // Если включен режим проволочных граней для инстанса
                if instance.wireframe {
                    draw_triangle_wireframe(v0, v1, v2, &mut ctx);
                    continue;
                }

                // Нормаль грани в мировом пространстве
                let m = &instance.mesh;
                let (a, b, c) = (
                    m.vertices[triangle[0]],
                    m.vertices[triangle[1]],
                    m.vertices[triangle[2]],
                );
                let n_local = (b - a).cross(&(c - a)).normalize();
                let n = (&model_matrix * n_local).to_vec3_dir().normalize();

                // 0.25 — фоновая подсветка, чтобы тень не была чёрной
                let intensity = 0.25 + 0.75 * n.dot(&light_dir).max(0.0);

                ctx.color = [
                    (base_color[0] as f32 * intensity) as u8,
                    (base_color[1] as f32 * intensity) as u8,
                    (base_color[2] as f32 * intensity) as u8,
                    base_color[3],
                ];

                // Режем по ближней плоскости и растеризуем осколки
                let (triangles, n) = clip_triangle_near([v0, v1, v2]);
                for triangle in &triangles[..n] {
                    draw_triangle_filled(triangle[0], triangle[1], triangle[2], &mut ctx);
                }
            }
        }
    }
}
