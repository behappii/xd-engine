use crate::math::{Vec2, Vec3};

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
