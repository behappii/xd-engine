use rayon::prelude::*;

use crate::{
    clipping::clip_triangle_near,
    config::{
        AMBIENT_LIGHT, DEFAULT_FAR, DEFAULT_FOV, DEFAULT_NEAR, LIGHT_DIRECTION, LINE_COLOR,
        RASTER_BAND_ROWS,
    },
    math::{Mat4, Vec2, Vec3, Vec4},
    renderer::{
        DrawContext, ShadedVertex, draw_triangle_filled, draw_triangle_wireframe, is_backface,
    },
    texture::Texture,
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
/// пайплайну вместе с ней — сюда же со временем добавится цвет вершины.
#[derive(Debug, Clone, Copy)]
pub struct Vertex {
    pub position: Vec3,
    pub normal: Vec3,
    /// Развёртка: какая точка текстуры приклеена к этой вершине.
    /// У мешей без развёртки — нули
    pub uv: Vec2,
}

impl Vertex {
    pub fn new(position: Vec3, normal: Vec3) -> Self {
        Self {
            position,
            normal,
            uv: Vec2::ZERO,
        }
    }

    pub fn with_uv(mut self, uv: Vec2) -> Self {
        self.uv = uv;
        self
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
        Self::flat_shaded_uv(positions, triangles, &[])
    }

    /// То же самое, но с развёрткой: `uvs[i]` — текстурные координаты трёх
    /// углов треугольника `triangles[i]`.
    ///
    /// UV задаётся ПО ТРЕУГОЛЬНИКАМ, а не по позициям, и это не прихоть.
    /// Развёртка почти всегда рвётся там же, где и нормаль: угол куба на
    /// картинке-развёртке — это три разные точки, по одной на грань. Раз
    /// вершины здесь и так расщепляются, дать каждой копии своё UV ничего
    /// не стоит, а привязка к общей позиции сделала бы это невозможным.
    ///
    /// Короткий (или пустой) список — не ошибка: у треугольников без записи
    /// UV остаётся нулевым. Так `flat_shaded` продолжает работать как раньше
    pub fn flat_shaded_uv(positions: &[Vec3], triangles: &[[usize; 3]], uvs: &[[Vec2; 3]]) -> Self {
        let mut out_vertices = Vec::with_capacity(triangles.len() * 3);
        let mut out_triangles = Vec::with_capacity(triangles.len());

        for (i, triangle) in triangles.iter().enumerate() {
            let a = positions[triangle[0]];
            let b = positions[triangle[1]];
            let c = positions[triangle[2]];

            // Обход против часовой стрелки снаружи -> нормаль наружу
            let normal = (b - a).cross(&(c - a)).normalize();

            let uv = uvs.get(i).copied().unwrap_or([Vec2::ZERO; 3]);

            let base = out_vertices.len();

            out_vertices.push(Vertex::new(a, normal).with_uv(uv[0]));
            out_vertices.push(Vertex::new(b, normal).with_uv(uv[1]));
            out_vertices.push(Vertex::new(c, normal).with_uv(uv[2]));

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
        let positions = [
            Vec3::new(-1.0, -1.0, -1.0), // 0
            Vec3::new(1.0, -1.0, -1.0),  // 1
            Vec3::new(1.0, 1.0, -1.0),   // 2
            Vec3::new(-1.0, 1.0, -1.0),  // 3
            Vec3::new(-1.0, -1.0, 1.0),  // 4
            Vec3::new(1.0, -1.0, 1.0),   // 5
            Vec3::new(1.0, 1.0, 1.0),    // 6
            Vec3::new(-1.0, 1.0, 1.0),   // 7
        ];

        // Грань задаётся четвёркой углов, а не двумя тройками, потому что
        // развёртка мыслит четырёхугольниками: каждой грани отдаётся весь
        // квадрат текстуры целиком.
        //
        // Порядок углов — против часовой стрелки при взгляде СНАРУЖИ, начиная
        // с левого нижнего угла картинки на этой грани. Первое условие даёт
        // нормаль наружу (иначе грань отбракует `is_backface`), второе — что
        // текстура на грани стоит ровно, а не боком. Проверять их приходится
        // руками: перепутанный угол не вызовет ошибки, он просто повернёт
        // картинку, и заметно это будет только на несимметричной текстуре
        const FACES: [[usize; 4]; 6] = [
            [1, 0, 3, 2], // Back (-Z): смотрим со стороны -Z, «вправо» = -X
            [4, 5, 6, 7], // Front (+Z)
            [0, 1, 5, 4], // Bottom (-Y): «вверх» картинки = +Z
            [7, 6, 2, 3], // Top (+Y): «вверх» картинки = -Z
            [0, 4, 7, 3], // Left (-X)
            [5, 1, 2, 6], // Right (+X)
        ];

        // Углы квадрата текстуры в том же порядке. v растёт вниз, поэтому
        // левый НИЖНИЙ угол грани — это v = 1
        const QUAD_UV: [Vec2; 4] = [
            Vec2::new(0.0, 1.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(0.0, 0.0),
        ];

        let mut triangles = Vec::with_capacity(12);
        let mut uvs = Vec::with_capacity(12);

        for face in FACES {
            // Четырёхугольник режется по диагонали a-c: оба треугольника
            // начинаются с общего угла a
            triangles.push([face[0], face[1], face[2]]);
            triangles.push([face[0], face[2], face[3]]);

            uvs.push([QUAD_UV[0], QUAD_UV[1], QUAD_UV[2]]);
            uvs.push([QUAD_UV[0], QUAD_UV[2], QUAD_UV[3]]);
        }

        Self::flat_shaded_uv(&positions, &triangles, &uvs)
    }

    /// UV-сфера радиуса 1: `stacks` колец по широте, `slices` долей по долготе.
    ///
    /// Строится как обычная прямоугольная сетка `(stacks + 1) × (slices + 1)`
    /// вершин — та самая, в которой рисуют карты мира. Оба «+1» здесь не
    /// описка, а ровно то, что отличает сферу С развёрткой от сферы без неё:
    ///
    /// - **Лишний столбец по долготе** — это шов. Меридиан 0° и меридиан 360° —
    ///   одна и та же точка пространства, но на картинке это левый и правый
    ///   края, `u = 0` и `u = 1`. Одной вершиной два значения не выразить,
    ///   поэтому шовный столбец дублируется. Раньше сфера, наоборот,
    ///   замыкала кольцо остатком от деления и вершин не дублировала.
    /// - **Лишняя строка по широте** — полюса. У полюса вся строка сетки
    ///   схлопывается в одну точку, но каждой доле нужна своя вершина, потому
    ///   что `u` у них разное.
    ///
    /// Раньше дублировать вершины было нельзя: нормали считал `smooth_shaded`
    /// усреднением по сходящимся граням, и у копии на шве соседей оказывалась
    /// половина — по сфере пошла бы видимая полоса. Здесь нормали берутся не
    /// усреднением, а точно: у сферы единичного радиуса нормаль в точке равна
    /// самой позиции. Копии получают одинаковую нормаль по построению, и шов
    /// исчезает из затенения сам собой
    pub fn create_sphere(stacks: usize, slices: usize) -> Self {
        let stacks = stacks.max(2);
        let slices = slices.max(3);

        let mut vertices = Vec::with_capacity((stacks + 1) * (slices + 1));

        for stack in 0..=stacks {
            // phi отсчитывается от северного полюса: 0 наверху, PI внизу
            let phi = std::f32::consts::PI * stack as f32 / stacks as f32;
            let (y, radius) = (phi.cos(), phi.sin());

            // v растёт вниз, как строки картинки, поэтому северный полюс —
            // верхний край текстуры. Так же лежат и обычные карты мира
            let v = stack as f32 / stacks as f32;
            let at_pole = stack == 0 || stack == stacks;

            for slice in 0..=slices {
                let theta = std::f32::consts::TAU * slice as f32 / slices as f32;

                let position = Vec3::new(radius * theta.cos(), y, radius * theta.sin());

                // У полюса четырёхугольник вырождается в треугольник, и его
                // вершине достаётся не край доли, а её середина: иначе
                // текстурный треугольник выходит прямоугольным вместо
                // равнобедренного и картинка у полюсов заметно косит
                let u = if at_pole {
                    (slice as f32 + 0.5) / slices as f32
                } else {
                    slice as f32 / slices as f32
                };

                // Позиция на единичной сфере уже единичной длины, и она же —
                // точная нормаль. Усреднять по граням незачем: усреднение лишь
                // приближает то, что здесь известно из формулы
                vertices.push(Vertex::new(position, position).with_uv(Vec2::new(u, v)));
            }
        }

        // Сетка хранится строками, поэтому шаг по широте — вся длина строки
        let index = |stack: usize, slice: usize| stack * (slices + 1) + slice;

        let mut triangles = Vec::with_capacity(stacks * slices * 2);

        for stack in 0..stacks {
            for slice in 0..slices {
                let a = index(stack, slice);
                let b = index(stack, slice + 1);
                let c = index(stack + 1, slice);
                let d = index(stack + 1, slice + 1);

                if stack == 0 {
                    // Северная шапка: a и b — обе полюс, треугольник [a, b, c]
                    // выродился бы в отрезок нулевой площади. Остаётся один
                    // треугольник, и его вершина берётся из столбца slice —
                    // именно там лежит середина доли, посчитанная выше
                    triangles.push([a, d, c]);
                } else {
                    triangles.push([a, b, c]);

                    // Южная шапка симметрична: там вырождается [b, d, c],
                    // потому что полюс уже c и d
                    if stack + 1 != stacks {
                        triangles.push([b, d, c]);
                    }
                }
            }
        }

        Self {
            vertices,
            triangles,
        }
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

/// Ссылка на меш, живущий в сцене.
///
/// Внутри обычный индекс, поэтому тип `Copy`: объявил меш один раз, дальше
/// раздавай сколько угодно инстансам без всяких `clone`. Раньше на этом месте
/// был `Rc<Mesh>`, и у него было ровно два минуса. Пользовательский: чтобы
/// переиспользовать меш, приходилось писать `Rc::new` и `Rc::clone` руками.
/// И технический, куда более неприятный: `Rc` не `Sync`, потому что счётчик
/// ссылок у него неатомарный, а значит `&Instance` нельзя было отдать в
/// другой поток — вершинный этап оставался однопоточным.
///
/// Плата за простоту: типом ничего не гарантируется. Индекс из чужой сцены
/// скомпилируется и молча возьмёт не тот меш
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshId(usize);

/// Ссылка на текстуру, живущую в сцене. Всё то же, что и у [`MeshId`]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureId(usize);

pub struct Instance {
    pub mesh: MeshId,
    pub position: Vec3,
    pub rotation: Vec3,
    pub scale: Vec3,

    // цвет всего объекта
    pub color: [u8; 4], // R G B A
    // необязательная раскраска по треугольникам
    // индекс совпадает с mesh.triangles
    pub face_colors: Option<Vec<[u8; 4]>>,

    /// Текстура объекта, тоже ссылкой в арену сцены: одну картинку разделяют
    /// десятки инстансов, копировать её на каждый незачем
    pub texture: Option<TextureId>,

    pub wireframe: bool,
}

impl Instance {
    pub fn new(mesh: MeshId, position: Vec3) -> Self {
        Self {
            mesh,
            position,
            rotation: Vec3::new(0.0, 0.0, 0.0),
            scale: Vec3::new(1.0, 1.0, 1.0),
            color: LINE_COLOR,
            face_colors: None,
            texture: None,
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

    /// Натянуть текстуру.
    ///
    /// Цвет инстанса при этом не отменяется, а умножается на тексель: белый
    /// (255, 255, 255) отдаёт картинку как есть, любой другой её подкрашивает.
    /// Смысла в текстуре не будет, если у меша нет развёртки — тогда все UV
    /// нулевые и вся поверхность прочитает один и тот же левый верхний тексель
    pub fn with_texture(mut self, texture: TextureId) -> Self {
        self.texture = Some(texture);
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
    /// Арена мешей. Сцена ими владеет, инстансы держат только индексы —
    /// поэтому в сцене нет ни одного разделяемого указателя, и её целиком
    /// можно одолжить сразу нескольким потокам
    meshes: Vec<Mesh>,
    /// Арена текстур, по той же схеме
    textures: Vec<Texture>,
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
            meshes: Vec::new(),
            textures: Vec::new(),
            instances: Vec::new(),
            camera_position: Vec3::new(0.0, 0.0, 5.0),
            yaw: -90.0,
            pitch: 0.0,
        }
    }

    /// Отдать меш сцене и получить ссылку на него.
    ///
    /// Ссылку можно копировать сколько угодно: она `Copy`, и каждый инстанс
    /// хранит у себя всего одно число
    pub fn add_mesh(&mut self, mesh: Mesh) -> MeshId {
        self.meshes.push(mesh);

        MeshId(self.meshes.len() - 1)
    }

    /// То же для текстуры
    pub fn add_texture(&mut self, texture: Texture) -> TextureId {
        self.textures.push(texture);

        TextureId(self.textures.len() - 1)
    }

    pub fn mesh(&self, id: MeshId) -> &Mesh {
        &self.meshes[id.0]
    }

    /// Изменить меш на месте — например деформировать вершины.
    ///
    /// Правка достанется ВСЕМ инстансам с этим `MeshId`. Чтобы изменить
    /// только один объект, зарегистрируй копию: `let id = scene.add_mesh(
    /// scene.mesh(base).clone())`. Раньше это делал `Rc::make_mut`, который
    /// клонировал молча — теперь копия видна в коде
    pub fn mesh_mut(&mut self, id: MeshId) -> &mut Mesh {
        &mut self.meshes[id.0]
    }

    pub fn texture(&self, id: TextureId) -> &Texture {
        &self.textures[id.0]
    }

    pub fn add_instance(&mut self, instance: Instance) {
        self.instances.push(instance);
    }

    /// Короткий путь для меша, который нужен ровно одному объекту: регистрирует
    /// его и сразу заводит инстанс. Возвращает ссылку на инстанс, чтобы можно
    /// было донастроить масштаб и поворот
    pub fn spawn(&mut self, mesh: Mesh, position: Vec3) -> &mut Instance {
        let id = self.add_mesh(mesh);
        self.instances.push(Instance::new(id, position));

        self.instances.last_mut().expect("только что добавили")
    }

    /// Отрисовать кадр, распараллелив растеризацию по числу ядер.
    /// Отрисовать кадр, разложив работу по глобальному пулу потоков rayon.
    ///
    /// Пул создаётся один раз на процесс и живёт между кадрами. Это важнее,
    /// чем кажется: раньше здесь был `thread::scope`, и потоки создавались
    /// заново каждый кадр — на 12 потоках это стоило около 0.2 мс, то есть
    /// больше, чем весь вершинный этап
    pub fn draw(&self, frame: &mut [u8], depth: &mut [f32], width: u32, height: u32) {
        // Геометрия считается один раз на кадр, а не в каждой полосе: полос
        // десятки, и повторять вершинный этап для каждой было бы вернейшим
        // способом сделать «многопоточность», которая медленнее исходника
        let jobs = self.build_raster_jobs(width, height, true);

        rasterize(frame, depth, width, height, &jobs, true);
    }

    /// Отрисовать кадр строго в один поток, вообще не трогая пул.
    ///
    /// Это опорная точка для сравнения: кадр, собранный так, обязан
    /// ПОБАЙТОВО совпасть с параллельным. Полосы не пересекаются, каждый
    /// пиксель пишет ровно один поток, и порядок треугольников внутри полосы
    /// тот же — значит совпадение должно быть точным, а не «на глаз».
    /// Расхождение означает гонку
    pub fn draw_serial(&self, frame: &mut [u8], depth: &mut [f32], width: u32, height: u32) {
        let jobs = self.build_raster_jobs(width, height, false);

        rasterize(frame, depth, width, height, &jobs, false);
    }

    /// Отрисовать кадр на пуле ровно из `threads` потоков.
    ///
    /// Нужно для тестов и замеров: обычный `draw` берёт глобальный пул, число
    /// потоков в котором задаёт rayon по числу ядер. Пул здесь строится на
    /// один вызов, так что для горячего пути это не годится
    pub fn draw_with_threads(
        &self,
        frame: &mut [u8],
        depth: &mut [f32],
        width: u32,
        height: u32,
        threads: usize,
    ) {
        if threads <= 1 {
            return self.draw_serial(frame, depth, width, height);
        }

        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .expect("не удалось собрать пул потоков")
            .install(|| self.draw(frame, depth, width, height));
    }

    /// Вершинный этап целиком: из инстансов получается плоский список
    /// треугольников, готовых к растеризации.
    ///
    /// Раньше этот код рисовал сразу, по ходу обхода инстансов. Разделение
    /// нужно потому, что растеризация теперь идёт по полосам экрана, а полоса
    /// не знает ничего про инстансы — ей нужен готовый список. Порядок списка
    /// повторяет прежний порядок отрисовки, и это важно: тест глубины и
    /// затирание проволочных линий зависят от порядка
    fn build_raster_jobs(&self, width: u32, height: u32, parallel: bool) -> Vec<RasterJob<'_>> {
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

        // Направление на источник света — одно на всю сцену, считаем до циклов
        let light_dir = LIGHT_DIRECTION.normalize();

        // У каждого инстанса свой выходной вектор, и складываются они потом
        // строго по порядку инстансов. Это и есть весь секрет сохранения
        // порядка: как бы инстансы ни разошлись по потокам, склейка идёт по
        // индексу, а не по тому, кто раньше закончил. Порядок важен, потому
        // что от него зависят тест глубины и затирание проволочных линий
        let mut per_instance: Vec<Vec<RasterJob<'_>>> =
            (0..self.instances.len()).map(|_| Vec::new()).collect();

        if parallel {
            // Именно здесь окупился отказ от Rc: в сцене не осталось ни одного
            // неатомарного счётчика ссылок, поэтому `&Scene` — Sync, и её можно
            // просто одолжить всем потокам.
            //
            // for_each_init, а не for_each: черновики создаются ОДИН раз на
            // рабочий поток и переиспользуются между его инстансами. Обычный
            // for_each выделял бы их заново на каждый объект каждый кадр
            self.instances
                .par_iter()
                .zip(per_instance.par_iter_mut())
                .for_each_init(VertexScratch::default, |scratch, (instance, out)| {
                    self.shade_instance(instance, &vp_matrix, light_dir, scratch, out);
                });
        } else {
            let mut scratch = VertexScratch::default();

            for (instance, out) in self.instances.iter().zip(per_instance.iter_mut()) {
                self.shade_instance(instance, &vp_matrix, light_dir, &mut scratch, out);
            }
        }

        let mut jobs: Vec<RasterJob<'_>> =
            Vec::with_capacity(per_instance.iter().map(Vec::len).sum());

        for mut chunk in per_instance {
            jobs.append(&mut chunk);
        }

        jobs
    }

    /// Вершинный этап одного инстанса: из меша получаются готовые треугольники.
    ///
    /// Вынесено в отдельный метод, чтобы одинаково вызываться и из
    /// однопоточной ветки, и из потока
    fn shade_instance<'scene>(
        &'scene self,
        instance: &Instance,
        vp_matrix: &Mat4,
        light_dir: Vec3,
        scratch: &mut VertexScratch,
        out: &mut Vec<RasterJob<'scene>>,
    ) {
        let mesh = self.mesh(instance.mesh);

        let model_matrix = instance.get_model_matrix();
        let mvp_matrix = vp_matrix * &model_matrix;

        // Текстура — состояние на весь инстанс, как и на GPU: она
        // «привязывается» один раз перед отрисовкой объекта. Здесь она всё же
        // копируется в каждый треугольник: список плоский, инстанс из него
        // уже не виден, а `Option<&Texture>` — это одно слово
        let texture = instance.texture.map(|id| self.texture(id));

        // Вершинный этап: позиция уходит в clip space, а нормаль сразу
        // превращается в яркость. Это и есть затенение по Гуро — свет
        // считается в вершинах, дальше по грани его протянет интерполяция.
        // Сама нормаль ниже уже не нужна, поэтому и не храним её
        let VertexScratch {
            clip_vertices,
            intensities,
        } = scratch;

        clip_vertices.clear();
        intensities.clear();

        for vertex in &mesh.vertices {
            clip_vertices.push(&mvp_matrix * vertex.position);

            let normal = model_matrix.transform_dir(vertex.normal).normalize();
            let lambert = normal.dot(&light_dir).max(0.0);

            intensities.push(AMBIENT_LIGHT + (1.0 - AMBIENT_LIGHT) * lambert);
        }

        // Отрисовываем грани этого меша с отсечением невидимых
        {
            for (i, triangle) in mesh.triangles.iter().enumerate() {
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

                // Если включен режим проволочных граней для инстанса.
                // Проволока режется по ближней плоскости не здесь, а внутри
                // самой отрисовки линии, поэтому в список едут исходные
                // clip-позиции
                if instance.wireframe {
                    out.push(RasterJob::Wireframe {
                        positions: [v0, v1, v2],
                        color: base_color,
                    });
                    continue;
                }

                // Собираем вершины для растеризатора: у каждой свой цвет,
                // потому что своя яркость. У меша из flat_shaded все три
                // яркости совпадают и грань выходит однотонной, у гладкого —
                // расходятся, и интерполяция даёт градиент
                let base = unpack_color(base_color);
                let shaded = |index: usize| {
                    ShadedVertex::new(clip_vertices[index], base * intensities[index])
                        .with_uv(mesh.vertices[index].uv)
                };

                // Режем по ближней плоскости; в список едут уже осколки
                let (triangles, count) = clip_triangle_near([
                    shaded(triangle[0]),
                    shaded(triangle[1]),
                    shaded(triangle[2]),
                ]);

                for triangle in &triangles[..count] {
                    out.push(RasterJob::Filled {
                        vertices: *triangle,
                        texture,
                    });
                }
            }
        }
    }
}

/// Переиспользуемые буферы вершинного этапа.
///
/// Один такой на поток, а не на инстанс: длина у них — число вершин меша, и
/// без переиспользования каждый объект каждый кадр заново выделял бы память
#[derive(Default)]
struct VertexScratch {
    clip_vertices: Vec<Vec4>,
    intensities: Vec<f32>,
}

/// Один готовый к растеризации треугольник.
///
/// Плоский список таких работ — это граница между вершинным этапом и
/// растеризацией. Всё, что нужно знать про инстанс, здесь уже скопировано:
/// потоку-растеризатору сцена не видна вообще
enum RasterJob<'tex> {
    Filled {
        vertices: [ShadedVertex; 3],
        texture: Option<&'tex Texture>,
    },
    /// У проволоки нет ни атрибутов, ни теста глубины — только позиции и цвет
    Wireframe {
        positions: [Vec4; 3],
        color: [u8; 4],
    },
}

/// Полоса кадра, отданная одному потоку: номер первой строки и куски обоих
/// буферов, относящиеся только к ней
type Band<'a> = (u32, &'a mut [u8], &'a mut [f32]);

/// Растеризовать список треугольников в кадр, разложив работу по потокам.
///
/// Схема: кадр режется на горизонтальные полосы, полосы разбирает пул
/// потоков, каждая полоса проходит ВЕСЬ список треугольников и рисует только
/// то, что в неё попало.
///
/// Раздачей занимается work-stealing rayon, а не мы: поток, доевший свои
/// полосы, забирает чужие. Раньше здесь была статическая раздача по кругу —
/// она нужна была потому, что сплошной кусок экрана даёт неравномерную
/// нагрузку (небо сверху пустое, пол снизу закрашен целиком). Кража работы
/// решает ту же задачу лучше и без наших рук: перекос выравнивается по факту,
/// а не по догадке о том, где на экране будет тяжело.
///
/// Ключевое свойство — полосы не пересекаются. Значит два потока физически не
/// могут писать в один пиксель, и никакой синхронизации, атомиков и мьютексов
/// не нужно: `chunks_mut` выдаёт непересекающиеся `&mut`-срезы, и это
/// доказывает компилятор, а не комментарий.
///
/// Цена схемы — треугольник обходят все потоки, даже те, чьи полосы он не
/// задевает. Отсечение по Y стоит несколько сравнений, так что для крупных
/// граней это ничто, а вот на сцене из множества мелких треугольников
/// начнёт мешать: лечится разбиением экрана на плитки с предварительной
/// раскладкой треугольников по ним, но это заметно сложнее
fn rasterize(
    frame: &mut [u8],
    depth: &mut [f32],
    width: u32,
    height: u32,
    jobs: &[RasterJob<'_>],
    parallel: bool,
) {
    if width == 0 || height == 0 {
        return;
    }

    let band_pixels = RASTER_BAND_ROWS * width as usize;

    let bands: Vec<Band<'_>> = frame
        .chunks_mut(band_pixels * 4)
        .zip(depth.chunks_mut(band_pixels))
        .enumerate()
        .map(|(i, (frame_band, depth_band))| {
            ((i * RASTER_BAND_ROWS) as u32, frame_band, depth_band)
        })
        .collect();

    if parallel {
        bands
            .into_par_iter()
            .for_each(|band| rasterize_band(band, jobs, width, height));
    } else {
        // Путь без единого потока: с ним сравнивается параллельный результат
        for band in bands {
            rasterize_band(band, jobs, width, height);
        }
    }
}

/// Пройти весь список треугольников для одной полосы
fn rasterize_band(band: Band<'_>, jobs: &[RasterJob<'_>], width: u32, height: u32) {
    let (y_offset, frame_band, depth_band) = band;

    // Высота берётся из длины среза, а не из RASTER_BAND_ROWS: последняя
    // полоса короче, если высота кадра не делится нацело
    let rows = depth_band.len() as u32 / width;

    let mut ctx = DrawContext::band(
        frame_band, depth_band, width, height, y_offset, rows, LINE_COLOR,
    );

    for job in jobs {
        match job {
            RasterJob::Filled { vertices, texture } => {
                ctx.texture = *texture;
                draw_triangle_filled(vertices[0], vertices[1], vertices[2], &mut ctx);
            }
            RasterJob::Wireframe { positions, color } => {
                ctx.color = *color;
                draw_triangle_wireframe(positions[0], positions[1], positions[2], &mut ctx);
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
