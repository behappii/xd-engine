use std::rc::Rc;

use crate::{
    clipping::clip_triangle_near,
    config::{
        AMBIENT_LIGHT, DEFAULT_FAR, DEFAULT_FOV, DEFAULT_NEAR, LIGHT_DIRECTION, LINE_COLOR,
    },
    math::{Mat4, Vec3, Vec4},
    renderer::{
        DrawContext, ShadedVertex, draw_triangle_filled, draw_triangle_wireframe, is_backface,
    },
};

/// Цвет из байтов в вектор с компонентами 0..1.
/// В таком виде его можно умножать на яркость и интерполировать
fn unpack_color(color: [u8; 4]) -> Vec3 {
    Vec3::new(
        color[0] as f32 / 255.0,
        color[1] as f32 / 255.0,
        color[2] as f32 / 255.0,
    )
}

/// Вершина со всеми своими атрибутами.
///
/// Раньше меш хранил голые позиции, а нормаль считалась в цикле отрисовки
/// заново на каждый кадр. Теперь атрибут живёт рядом с позицией и едет по
/// пайплайну вместе с ней — сюда же со временем добавятся UV и цвет.
#[derive(Debug, Clone, Copy)]
pub struct Vertex {
    pub position: Vec3,
    pub normal: Vec3,
}

impl Vertex {
    pub fn new(position: Vec3, normal: Vec3) -> Self {
        Self { position, normal }
    }
}

#[derive(Clone, Debug)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub triangles: Vec<[usize; 3]>,
}

impl Mesh {
    /// Собрать меш с плоским затенением из «сырой» геометрии:
    /// список позиций + список треугольников-индексов.
    ///
    /// Каждый треугольник получает СВОИ три вершины, а общие позиции
    /// дублируются. Причина в том, что нормаль теперь атрибут вершины, а у
    /// угла куба нормали три — по одной на сходящуюся грань, — и одной
    /// вершиной их не выразить. Это фундаментальная плата за резкие рёбра:
    /// вершины приходится «расщеплять» везде, где атрибут терпит разрыв.
    ///
    /// Куб из-за этого распухает с 8 позиций до 36 вершин. Для гладких
    /// поверхностей (сферы, ландшафта) вершины, наоборот, разделяются между
    /// гранями — там индексный буфер и начинает окупаться.
    pub fn flat_shaded(positions: &[Vec3], triangles: &[[usize; 3]]) -> Self {
        let mut out_vertices = Vec::with_capacity(triangles.len() * 3);
        let mut out_triangles = Vec::with_capacity(triangles.len());

        for triangle in triangles {
            let a = positions[triangle[0]];
            let b = positions[triangle[1]];
            let c = positions[triangle[2]];

            // Обход против часовой стрелки снаружи -> нормаль наружу
            let normal = (b - a).cross(&(c - a)).normalize();

            let base = out_vertices.len();

            out_vertices.push(Vertex::new(a, normal));
            out_vertices.push(Vertex::new(b, normal));
            out_vertices.push(Vertex::new(c, normal));

            out_triangles.push([base, base + 1, base + 2]);
        }

        Self {
            vertices: out_vertices,
            triangles: out_triangles,
        }
    }

    /// Собрать меш с ГЛАДКИМ затенением: вершины остаются общими, а нормаль
    /// каждой — усреднённая по всем сходящимся в ней граням.
    ///
    /// Полная противоположность `flat_shaded`. Там вершины расщеплялись, чтобы
    /// на ребре получился разрыв нормали и стык был виден; здесь нормаль
    /// непрерывна, и затенение по Гуро размажет её по поверхности так, что
    /// гранёность пропадёт. Индексный буфер тут наконец работает по назначению:
    /// вершин ровно столько, сколько позиций.
    ///
    /// Векторные произведения складываются НЕнормализованными: длина такого
    /// вектора равна удвоенной площади грани, поэтому крупные грани
    /// автоматически получают больший вес в среднем
    pub fn smooth_shaded(positions: &[Vec3], triangles: &[[usize; 3]]) -> Self {
        let mut normals = vec![Vec3::new(0.0, 0.0, 0.0); positions.len()];

        for triangle in triangles {
            let a = positions[triangle[0]];
            let b = positions[triangle[1]];
            let c = positions[triangle[2]];

            let weighted = (b - a).cross(&(c - a));

            for index in triangle {
                normals[*index] = normals[*index] + weighted;
            }
        }

        let vertices = positions
            .iter()
            .zip(normals)
            .map(|(position, normal)| Vertex::new(*position, normal.normalize()))
            .collect();

        Self {
            vertices,
            triangles: triangles.to_vec(),
        }
    }

    /// Пересчитать нормали по текущим позициям.
    ///
    /// Нормали предвычислены, поэтому после ручной правки `vertices[i].position`
    /// они устаревают и освещение начинает врать. Этот метод их чинит.
    pub fn recalculate_flat_normals(&mut self) {
        for i in 0..self.triangles.len() {
            // Копия, а не ссылка: иначе self одолжен на чтение и вершины не поправить
            let triangle = self.triangles[i];

            let a = self.vertices[triangle[0]].position;
            let b = self.vertices[triangle[1]].position;
            let c = self.vertices[triangle[2]].position;

            let normal = (b - a).cross(&(c - a)).normalize();

            for index in triangle {
                self.vertices[index].normal = normal;
            }
        }
    }

    pub fn create_cube() -> Self {
        Self::flat_shaded(
            &[
                Vec3::new(-1.0, -1.0, -1.0), // 0
                Vec3::new(1.0, -1.0, -1.0),  // 1
                Vec3::new(1.0, 1.0, -1.0),   // 2
                Vec3::new(-1.0, 1.0, -1.0),  // 3
                Vec3::new(-1.0, -1.0, 1.0),  // 4
                Vec3::new(1.0, -1.0, 1.0),   // 5
                Vec3::new(1.0, 1.0, 1.0),    // 6
                Vec3::new(-1.0, 1.0, 1.0),   // 7
            ],
            &[
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
        )
    }

    /// UV-сфера радиуса 1: `stacks` колец по широте, `slices` долей по долготе.
    ///
    /// Полюса — по одной вершине, а кольца замкнуты по долготе, то есть шва из
    /// дублирующихся вершин нет. Это принципиально для `smooth_shaded`: если бы
    /// вершины на стыке дублировались, каждая копия усредняла бы нормаль лишь
    /// по половине соседей, и на сфере проступил бы шов
    pub fn create_sphere(stacks: usize, slices: usize) -> Self {
        let stacks = stacks.max(2);
        let slices = slices.max(3);

        let mut positions = Vec::with_capacity(2 + (stacks - 1) * slices);

        positions.push(Vec3::new(0.0, 1.0, 0.0));

        for stack in 1..stacks {
            let phi = std::f32::consts::PI * stack as f32 / stacks as f32;
            let (y, radius) = (phi.cos(), phi.sin());

            for slice in 0..slices {
                let theta = std::f32::consts::TAU * slice as f32 / slices as f32;

                positions.push(Vec3::new(radius * theta.cos(), y, radius * theta.sin()));
            }
        }

        positions.push(Vec3::new(0.0, -1.0, 0.0));

        let north = 0;
        let south = positions.len() - 1;

        // Замыкание по долготе остатком от деления — 359° соседствует с 0°
        let ring = |stack: usize, slice: usize| 1 + (stack - 1) * slices + slice % slices;

        let mut triangles = Vec::new();

        // Треугольные шапки у полюсов
        for slice in 0..slices {
            triangles.push([north, ring(1, slice + 1), ring(1, slice)]);
            triangles.push([south, ring(stacks - 1, slice), ring(stacks - 1, slice + 1)]);
        }

        // Четырёхугольные пояса между кольцами, каждый из двух треугольников
        for stack in 1..stacks - 1 {
            for slice in 0..slices {
                let a = ring(stack, slice);
                let b = ring(stack, slice + 1);
                let c = ring(stack + 1, slice);
                let d = ring(stack + 1, slice + 1);

                triangles.push([a, b, c]);
                triangles.push([b, d, c]);
            }
        }

        Self::smooth_shaded(&positions, &triangles)
    }

    pub fn create_pyramid() -> Self {
        Self::flat_shaded(
            &[
                Vec3::new(-1.0, -1.0, -1.0), // 0
                Vec3::new(-1.0, -1.0, 1.0),  // 1
                Vec3::new(1.0, -1.0, -1.0),  // 2
                Vec3::new(1.0, -1.0, 1.0),   // 3
                Vec3::new(0.0, 1.0, 0.0),    // 4
            ],
            &[
                // Основание
                [0, 2, 3],
                [0, 3, 1],
                // Боковые грани
                [0, 4, 2],
                [2, 4, 3],
                [3, 4, 1],
                [1, 4, 0],
            ],
        )
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

        // Направление на источник света — одно на всю сцену, считаем до циклов
        let light_dir = LIGHT_DIRECTION.normalize();

        // Буферы обработанных вершин переиспользуются между инстансами:
        // ёмкость выделяется один раз, а не на каждый объект каждый кадр
        let mut clip_vertices: Vec<Vec4> = Vec::new();
        let mut intensities: Vec<f32> = Vec::new();

        // Рендеринг каждого инстанса сцены
        for instance in &self.instances {
            let model_matrix = instance.get_model_matrix();
            let mvp_matrix = &vp_matrix * &model_matrix;

            // Вершинный этап: позиция уходит в clip space, а нормаль сразу
            // превращается в яркость. Это и есть затенение по Гуро — свет
            // считается в вершинах, дальше по грани его протянет интерполяция.
            // Сама нормаль ниже уже не нужна, поэтому и не храним её
            clip_vertices.clear();
            intensities.clear();

            for vertex in &instance.mesh.vertices {
                clip_vertices.push(&mvp_matrix * vertex.position);

                let normal = model_matrix.transform_dir(vertex.normal).normalize();
                let lambert = normal.dot(&light_dir).max(0.0);

                intensities.push(AMBIENT_LIGHT + (1.0 - AMBIENT_LIGHT) * lambert);
            }

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
                    ctx.color = base_color;
                    draw_triangle_wireframe(v0, v1, v2, &mut ctx);
                    continue;
                }

                // Собираем вершины для растеризатора: у каждой свой цвет,
                // потому что своя яркость. У меша из flat_shaded все три
                // яркости совпадают и грань выходит однотонной, у гладкого —
                // расходятся, и интерполяция даёт градиент
                let base = unpack_color(base_color);
                let shaded = |index: usize| {
                    ShadedVertex::new(clip_vertices[index], base * intensities[index])
                };

                // Режем по ближней плоскости и растеризуем осколки
                let (triangles, count) = clip_triangle_near([
                    shaded(triangle[0]),
                    shaded(triangle[1]),
                    shaded(triangle[2]),
                ]);

                for triangle in &triangles[..count] {
                    draw_triangle_filled(triangle[0], triangle[1], triangle[2], &mut ctx);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-5;

    /// Для выпуклой фигуры с центром в начале координат нормаль грани должна
    /// смотреть в ту же сторону, что и её центр — иначе перепутан обход вершин
    fn assert_normals_point_outward(mesh: &Mesh) {
        for (i, triangle) in mesh.triangles.iter().enumerate() {
            let a = mesh.vertices[triangle[0]].position;
            let b = mesh.vertices[triangle[1]].position;
            let c = mesh.vertices[triangle[2]].position;

            let centroid = (a + b + c) * (1.0 / 3.0);
            let normal = mesh.vertices[triangle[0]].normal;

            assert!(
                normal.dot(&centroid) > 0.0,
                "грань {} смотрит внутрь: нормаль {:?}, центр {:?}",
                i,
                normal,
                centroid
            );
        }
    }

    #[test]
    fn flat_shaded_splits_every_shared_vertex() {
        let cube = Mesh::create_cube();

        // 8 позиций превратились в 36 вершин: общих между гранями больше нет,
        // потому что нормаль у каждой грани своя
        assert_eq!(cube.triangles.len(), 12);
        assert_eq!(cube.vertices.len(), 36);
    }

    #[test]
    fn cube_normals_point_outward() {
        assert_normals_point_outward(&Mesh::create_cube());
    }

    #[test]
    fn pyramid_normals_point_outward() {
        assert_normals_point_outward(&Mesh::create_pyramid());
    }

    #[test]
    fn all_three_vertices_of_a_flat_face_share_one_normal() {
        let cube = Mesh::create_cube();

        // На этом держится выбор world_normals[triangle[0]] в Scene::draw
        for triangle in &cube.triangles {
            let n0 = cube.vertices[triangle[0]].normal;

            for index in triangle {
                let d = cube.vertices[*index].normal - n0;
                assert!(d.length() < EPS);
            }
        }
    }

    #[test]
    fn cube_has_six_distinct_face_normals() {
        let cube = Mesh::create_cube();

        // 12 треугольников, но куб — 6 плоскостей: пары треугольников совпадают
        let mut distinct: Vec<Vec3> = Vec::new();

        for triangle in &cube.triangles {
            let n = cube.vertices[triangle[0]].normal;

            if !distinct.iter().any(|d| (*d - n).length() < EPS) {
                distinct.push(n);
            }
        }

        assert_eq!(distinct.len(), 6);
    }

    #[test]
    fn recalculate_flat_normals_follows_moved_positions() {
        let mut mesh = Mesh::flat_shaded(
            &[
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            &[[0, 1, 2]],
        );

        // Треугольник лежит в плоскости XY, нормаль вдоль +Z
        assert!((mesh.vertices[0].normal - Vec3::new(0.0, 0.0, 1.0)).length() < EPS);

        // Ставим его «на ребро»: третью вершину поднимаем по Z вместо Y
        mesh.vertices[2].position = Vec3::new(0.0, 0.0, 1.0);
        mesh.recalculate_flat_normals();

        // Теперь плоскость XZ, нормаль вдоль -Y
        assert!((mesh.vertices[0].normal - Vec3::new(0.0, -1.0, 0.0)).length() < EPS);
    }

    #[test]
    fn recalculate_flat_normals_updates_all_three_vertices() {
        let mut mesh = Mesh::create_cube();

        mesh.vertices[0].position.y -= 1.0;
        mesh.recalculate_flat_normals();

        // Устаревшая нормаль хотя бы на одной вершине грани — это шов
        // с неправильным светом, поэтому обновиться должны все три
        let triangle = mesh.triangles[0];
        let n0 = mesh.vertices[triangle[0]].normal;

        for index in triangle {
            assert!((mesh.vertices[index].normal - n0).length() < EPS);
        }
    }
}
