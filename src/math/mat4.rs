use super::{Vec3, Vec4};

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
    /// Обратите внимание: для НОРМАЛЕЙ этого мало. Сама по себе матрица
    /// годится только для поворотов и равномерного масштаба (после
    /// `.normalize()`), при неравномерном она даёт неправильное направление.
    /// Нормаль надо гнать через [`Mat4::normal_matrix`] — там же и объяснение,
    /// почему направление получается неправильным.
    pub fn transform_dir(&self, dir: Vec3) -> Vec3 {
        let c = self.cols;

        Vec3::new(
            c[0][0] * dir.x + c[1][0] * dir.y + c[2][0] * dir.z,
            c[0][1] * dir.x + c[1][1] * dir.y + c[2][1] * dir.z,
            c[0][2] * dir.x + c[1][2] * dir.y + c[2][2] * dir.z,
        )
    }

    /// Транспонирование: строки становятся столбцами.
    ///
    /// Само по себе нужно редко, но у ортонормированной матрицы (чистый
    /// поворот) транспонированная равна обратной, и на этом держится
    /// [`Mat4::normal_matrix`]
    pub fn transpose(&self) -> Self {
        let mut cols = [[0.0; 4]; 4];

        for col in 0..4 {
            for row in 0..4 {
                cols[col][row] = self.cols[row][col];
            }
        }

        Self { cols }
    }

    /// Обратная матрица: такая, что `M * M⁻¹ = E`. `None`, если её нет.
    ///
    /// Метод Гаусса–Жордана — тот же самый, которым обратную матрицу считают
    /// на бумаге. К исходной матрице приписывается единичная, и дальше обе
    /// преобразуются ОДНИМИ И ТЕМИ ЖЕ операциями над строками, пока слева не
    /// получится единичная. Что при этом окажется справа — и есть обратная:
    /// каждая операция над строкой — это домножение слева на какую-то матрицу,
    /// и раз их произведение превратило `M` в `E`, то оно и равно `M⁻¹`.
    ///
    /// Альтернатива — формула через присоединённую матрицу (16 миноров 3×3).
    /// Она короче и без ветвлений, поэтому её обычно и берут в библиотеках, но
    /// читается как набор магических чисел, а численно ведёт себя хуже:
    /// выбора главного элемента там нет в принципе.
    ///
    /// Стоимость — порядка сотни операций, и вызывается это раз на инстанс
    /// в кадр, а не на вершину, так что на фоне вершинного этапа не видно.
    pub fn inverse(&self) -> Option<Self> {
        // Порог «в столбце ничего нет». Абсолютный, и это сознательный
        // компромисс: матрицу с масштабом 1e-7 по всем осям он объявит
        // вырожденной, хотя формально она обратима. Для модельных матриц это
        // не проблема — объект такого размера не занимает ни одного пикселя, —
        // а альтернатива хуже: почти-ноль в знаменателе не даёт ошибки, он
        // молча возвращает правдоподобные числа с потерянными разрядами
        const SINGULAR: f32 = 1e-6;

        // Внутри удобнее строками (метод-то построчный), а хранение у нас
        // по столбцам — перекладываем: `a[строка][столбец]`
        let mut a = [[0.0f32; 4]; 4];
        let mut inv = [[0.0f32; 4]; 4];

        for row in 0..4 {
            for col in 0..4 {
                a[row][col] = self.cols[col][row];
            }

            inv[row][row] = 1.0;
        }

        for col in 0..4 {
            // Выбор главного элемента: на место ведущей ставим строку, где в
            // этом столбце самое большое по модулю число. На бумаге так делать
            // необязательно, а в f32 обязательно — делить придётся именно на
            // него, и чем он меньше, тем сильнее раздуваются ошибки остальных
            // разрядов. Ищем только среди строк ниже: верхние уже отработаны,
            // трогать их нельзя
            let (pivot_row, pivot_abs) = (col..4)
                .map(|row| (row, a[row][col].abs()))
                .max_by(|left, right| left.1.total_cmp(&right.1))
                .expect("диапазон непустой: col < 4");

            // Весь столбец нулевой — строки линейно зависимы, обратной нет.
            // Практически это масштаб 0 по какой-то оси: объект сплющен
            // в плоскость, и развернуть его обратно уже нечем
            if pivot_abs < SINGULAR {
                return None;
            }

            a.swap(col, pivot_row);
            inv.swap(col, pivot_row);

            // Нормируем ведущую строку, чтобы ведущий элемент стал единицей
            let scale = 1.0 / a[col][col];

            for k in 0..4 {
                a[col][k] *= scale;
                inv[col][k] *= scale;
            }

            // И вычитаем её из всех остальных так, чтобы в этом столбце у них
            // остался ноль. После четырёх таких проходов слева единичная
            for row in 0..4 {
                if row == col {
                    continue;
                }

                let factor = a[row][col];

                for k in 0..4 {
                    a[row][k] -= factor * a[col][k];
                    inv[row][k] -= factor * inv[col][k];
                }
            }
        }

        // Обратно из строк в столбцы
        let mut cols = [[0.0f32; 4]; 4];

        for row in 0..4 {
            for col in 0..4 {
                cols[col][row] = inv[row][col];
            }
        }

        Some(Self { cols })
    }

    /// Матрица для НОРМАЛЕЙ — обратная транспонированная к модельной.
    ///
    /// Нормаль — не обычное направление. Обычное направление задано само по
    /// себе, а нормаль задана поверхностью: она перпендикулярна КАЖДОМУ ребру,
    /// лежащему в этой поверхности. Именно это свойство и надо сохранить, а
    /// оно, вообще говоря, не сохраняется.
    ///
    /// Формально. Пусть рёбра едут по матрице `A`, а нормаль — по какой-то
    /// матрице `B`. Требуется `(B·n)·(A·e) = 0` для всех рёбер `e`, у которых
    /// `n·e = 0`. Раскроем скалярное произведение: `(B·n)·(A·e) = nᵀ·Bᵀ·A·e`.
    /// Чтобы это совпало с исходным `nᵀ·e` при любых `n` и `e`, нужно
    /// `Bᵀ·A = E`, то есть `B = (A⁻¹)ᵀ`.
    ///
    /// Отсюда сразу видно, почему до сих пор хватало самой модельной матрицы.
    /// У чистого поворота `A⁻¹ = Aᵀ`, значит `B = A` — буквально та же матрица.
    /// У равномерного масштаба `A = s·R`, значит `B = R/s`: направление то же,
    /// отличается только длина, а её съедает `normalize()`. И только при
    /// неравномерном масштабе они расходятся по-настоящему.
    ///
    /// На пальцах: сплющим фигуру по Y. Наклонная грань становится ПОЛОЖЕ,
    /// то есть её нормаль обязана повернуться К оси Y. Наивное умножение
    /// сплющивает вместе с гранью и саму нормаль, то есть кладёт её — ровно
    /// в противоположную сторону. Ошибка тем заметнее, чем сильнее масштаб.
    ///
    /// Трансляция не мешает: у обратной к `[A t; 0 1]` перенос стоит в
    /// последнем СТОЛБЦЕ, после транспонирования уезжает в последнюю СТРОКУ,
    /// а `transform_dir` смотрит только на левый верхний угол 3×3.
    ///
    /// Вырожденную матрицу (нулевой масштаб по какой-то оси) возвращаем как
    /// есть: у фигуры, сплющенной в плоскость, нормали и не определены, а
    /// ронять из-за этого кадр незачем — свет на ней всё равно будет неверный,
    /// зато конечный
    pub fn normal_matrix(&self) -> Self {
        match self.inverse() {
            Some(inverse) => inverse.transpose(),
            None => *self,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::testing::{EPS, assert_vec3_eq};

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

        assert_vec3_eq(
            Vec3::new(point.x, point.y, point.z),
            Vec3::new(10.0, -5.0, 4.0),
        );
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

    // --- обратная матрица и нормальная матрица ---

    fn assert_mat4_eq(actual: Mat4, expected: Mat4) {
        for col in 0..4 {
            for row in 0..4 {
                let d = actual.cols[col][row] - expected.cols[col][row];

                assert!(
                    d.abs() < EPS,
                    "[{col}][{row}]: ожидали {}, получили {}",
                    expected.cols[col][row],
                    actual.cols[col][row]
                );
            }
        }
    }

    /// Модельная матрица со ВСЕМИ тремя составляющими и заведомо неравномерным
    /// масштабом: именно на нём расходятся наивный перенос нормали и честный
    fn squashed_model() -> Mat4 {
        &Mat4::translation(3.0, -1.0, 7.0)
            * &(&Mat4::rotation_y(37.0) * &Mat4::scaling(1.0, 0.25, 1.0))
    }

    #[test]
    fn inverse_undoes_the_matrix() {
        let m = squashed_model();
        let inv = m.inverse().expect("матрица невырождена");

        // Определение обратной, буквально. Проверяем с обеих сторон: для
        // квадратных матриц это одно и то же, но ошибка в перекладывании
        // строк и столбцов сломала бы ровно одну из проверок
        assert_mat4_eq(&m * &inv, Mat4::identity());
        assert_mat4_eq(&inv * &m, Mat4::identity());
    }

    #[test]
    fn inverse_returns_the_point_where_it_was() {
        // Смысловая сторона того же: обратная матрица возвращает точку назад
        // вместе с переносом, а не только поворот с масштабом
        let m = squashed_model();
        let inv = m.inverse().expect("матрица невырождена");

        let point = Vec3::new(2.0, -3.0, 0.5);
        let there = &m * point;
        let back = &inv * Vec3::new(there.x, there.y, there.z);

        assert_vec3_eq(Vec3::new(back.x, back.y, back.z), point);
    }

    #[test]
    fn inverse_of_a_flattened_matrix_is_none() {
        // Масштаб 0 по Y — объект сплющен в плоскость. Информация о том, какой
        // он был толщины, потеряна безвозвратно, развернуть обратно нечем
        assert!(Mat4::scaling(1.0, 0.0, 1.0).inverse().is_none());
    }

    #[test]
    fn transpose_swaps_rows_and_columns() {
        let m = Mat4::translation(1.0, 2.0, 3.0);
        let t = m.transpose();

        // У translation перенос лежит в последнем столбце, после
        // транспонирования обязан оказаться в последней строке
        assert_eq!(t.cols[0][3], 1.0);
        assert_eq!(t.cols[1][3], 2.0);
        assert_eq!(t.cols[2][3], 3.0);
        assert_mat4_eq(t.transpose(), m);
    }

    #[test]
    fn normal_matrix_keeps_the_normal_perpendicular_to_the_surface() {
        // Главный тест всей затеи, и ожидание в нём считается из ОПРЕДЕЛЕНИЯ
        // нормали, а не повторением формулы: нормаль перпендикулярна рёбрам
        // поверхности — значит, обязана остаться перпендикулярной им и после
        // преобразования.
        //
        // Поверхность задаём двумя рёбрами, нормаль — их векторным
        // произведением; так же её считает и `Mesh::flat_shaded`
        let e1 = Vec3::new(1.0, 1.0, 0.0);
        let e2 = Vec3::new(0.0, 1.0, 1.0);
        let n = e1.cross(&e2).normalize();

        let model = squashed_model();

        // Рёбра — обычные направления, они едут по модельной матрице
        let e1_world = model.transform_dir(e1);
        let e2_world = model.transform_dir(e2);

        let n_world = model.normal_matrix().transform_dir(n).normalize();

        assert!(
            n_world.dot(&e1_world.normalize()).abs() < EPS,
            "нормаль перестала быть перпендикулярной первому ребру: {}",
            n_world.dot(&e1_world.normalize())
        );
        assert!(
            n_world.dot(&e2_world.normalize()).abs() < EPS,
            "нормаль перестала быть перпендикулярной второму ребру: {}",
            n_world.dot(&e2_world.normalize())
        );

        // И обратная сторона: наивный перенос нормали по модельной матрице
        // здесь ошибается, причём не на погрешность, а грубо. Иначе тест был
        // бы зелёным и на сломанном коде и ничего бы не стерёг
        let n_naive = model.transform_dir(n).normalize();

        assert!(
            n_naive.dot(&e1_world.normalize()).abs() > 0.3,
            "модельная матрица вдруг справилась сама — тест ничего не проверяет"
        );
    }

    #[test]
    fn normal_matrix_of_a_rotation_is_the_rotation_itself() {
        // Объяснение того, почему до сих пор всё работало: у ортонормированной
        // матрицы обратная равна транспонированной, значит обратная
        // транспонированная — это она сама
        let rotation = &Mat4::rotation_x(25.0) * &Mat4::rotation_y(-70.0);

        assert_mat4_eq(rotation.normal_matrix(), rotation);
    }

    #[test]
    fn normal_matrix_of_a_uniform_scale_only_changes_the_length() {
        // Второй случай, где расхождения нет: равномерный масштаб s даёт
        // множитель 1/s, а его убирает normalize()
        let model = &Mat4::translation(4.0, 0.0, -2.0) * &Mat4::scaling(3.0, 3.0, 3.0);
        let n = Vec3::new(0.0, 0.6, 0.8);

        assert_vec3_eq(
            model.normal_matrix().transform_dir(n).normalize(),
            model.transform_dir(n).normalize(),
        );
    }

    #[test]
    fn normal_matrix_of_a_flattened_model_does_not_produce_nan() {
        // Обратной нет, и вместо паники должно вернуться хоть что-то конечное:
        // свет на сплющенной в плоскость фигуре всё равно не определён
        let flat = Mat4::scaling(1.0, 0.0, 1.0).normal_matrix();
        let n = flat.transform_dir(Vec3::new(0.0, 1.0, 0.0));

        assert!(n.x.is_finite() && n.y.is_finite() && n.z.is_finite());
    }

    // --- умножение матриц ---

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
