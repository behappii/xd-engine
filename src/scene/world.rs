use crate::math::{Mat4, Vec3};

use super::{
    Assets, Instance, MeshId,
    pipeline::{build_raster_jobs, rasterize},
};

/// Мир: что где стоит и откуда на это смотрят.
///
/// Ресурсов не держит — только ссылки на них, поэтому сцен может быть сколько
/// угодно и переключаются они даром. Меню поверх игры рисуется двумя вызовами
/// `draw` в один и тот же буфер
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

    /// Куда смотрит камера — единичный вектор из `yaw`/`pitch`.
    ///
    /// `yaw` отсчитывается вокруг Y, `pitch` — подъём над горизонтом, оба в
    /// градусах. Обратите внимание на сборку вектора: `pitch` даёт Y напрямую
    /// (`sin`), а горизонтальную составляющую ужимает множителем `cos` — без
    /// него при взгляде вверх вектор перестал бы быть единичным и камера
    /// поехала бы тем сильнее, чем выше смотришь
    pub fn forward(&self) -> Vec3 {
        let yaw_rad = self.yaw.to_radians();
        let pitch_rad = self.pitch.to_radians();

        Vec3::new(
            yaw_rad.cos() * pitch_rad.cos(),
            pitch_rad.sin(),
            yaw_rad.sin() * pitch_rad.cos(),
        )
        .normalize()
    }

    /// Матрица вида — одна на кадр и ОДНА НА ОБА БЭКЕНДА.
    ///
    /// Раньше это лежало прямо в `build_raster_jobs`, и пока путь отрисовки
    /// был один, разницы не было. Теперь их два, и вторая копия этой формулы
    /// была бы худшим видом дубля: разъехавшись, две камеры дали бы два
    /// правдоподобных, но разных кадра — а искать причину пришлось бы в
    /// геометрии, в освещении, где угодно, только не в том, что один путь
    /// считает `pitch` чуть иначе.
    ///
    /// Проекция при этом у каждого своя, и это не непоследовательность:
    /// у CPU-пути NDC по глубине `[-1, 1]` (OpenGL), у Vulkan — `[0, 1]` и
    /// перевёрнутый Y. Общей может быть только та часть, которая описывает
    /// МИР, а не конвенции конкретного API
    pub(crate) fn view_matrix(&self) -> Mat4 {
        let target = self.camera_position + self.forward();
        let up = Vec3::new(0.0, 1.0, 0.0);

        Mat4::look_at(self.camera_position, target, up)
    }

    /// Завести инстанс и сразу получить ссылку на него.
    ///
    /// Нужно потому, что масштаб и поворот — это поля, а не методы-строители:
    /// без такой ссылки пришлось бы заводить временную переменную ради двух
    /// присваиваний
    pub fn spawn(&mut self, mesh: MeshId, position: Vec3) -> &mut Instance {
        self.instances.push(Instance::new(mesh, position));

        self.instances.last_mut().expect("только что добавили")
    }

    /// Отрисовать кадр, разложив работу по глобальному пулу потоков rayon.
    ///
    /// Пул создаётся один раз на процесс и живёт между кадрами. Это важнее,
    /// чем кажется: раньше здесь был `thread::scope`, и потоки создавались
    /// заново каждый кадр — на 12 потоках это стоило около 0.2 мс, то есть
    /// больше, чем весь вершинный этап
    pub fn draw(
        &self,
        assets: &Assets,
        frame: &mut [u8],
        depth: &mut [f32],
        width: u32,
        height: u32,
    ) {
        // Геометрия считается один раз на кадр, а не в каждой полосе: полос
        // десятки, и повторять вершинный этап для каждой было бы вернейшим
        // способом сделать «многопоточность», которая медленнее исходника
        let jobs = build_raster_jobs(self, assets, width, height, true);

        rasterize(frame, depth, width, height, &jobs, true);
    }

    /// Отрисовать кадр строго в один поток, вообще не трогая пул.
    ///
    /// Это опорная точка для сравнения: кадр, собранный так, обязан
    /// ПОБАЙТОВО совпасть с параллельным. Полосы не пересекаются, каждый
    /// пиксель пишет ровно один поток, и порядок треугольников внутри полосы
    /// тот же — значит совпадение должно быть точным, а не «на глаз».
    /// Расхождение означает гонку
    pub fn draw_serial(
        &self,
        assets: &Assets,
        frame: &mut [u8],
        depth: &mut [f32],
        width: u32,
        height: u32,
    ) {
        let jobs = build_raster_jobs(self, assets, width, height, false);

        rasterize(frame, depth, width, height, &jobs, false);
    }

    /// Отрисовать кадр на пуле ровно из `threads` потоков.
    ///
    /// Нужно для тестов и замеров: обычный `draw` берёт глобальный пул, число
    /// потоков в котором задаёт rayon по числу ядер. Пул здесь строится на
    /// один вызов, так что для горячего пути это не годится
    #[allow(clippy::too_many_arguments)]
    pub fn draw_with_threads(
        &self,
        assets: &Assets,
        frame: &mut [u8],
        depth: &mut [f32],
        width: u32,
        height: u32,
        threads: usize,
    ) {
        if threads <= 1 {
            return self.draw_serial(assets, frame, depth, width, height);
        }

        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .expect("не удалось собрать пул потоков")
            .install(|| self.draw(assets, frame, depth, width, height));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Заодно проверяет умолчание сцены: `yaw = -90` — это взгляд вдоль
    /// МИНУС Z, то есть «вперёд» в правой системе координат. Число это
    /// выглядит произвольным ровно до тех пор, пока не подставишь его в
    /// формулу, поэтому пусть проверка стоит рядом
    #[test]
    fn the_default_camera_looks_down_minus_z() {
        let forward = Scene::new().forward();

        assert!((forward.x - 0.0).abs() < 1e-6, "x = {}", forward.x);
        assert!((forward.y - 0.0).abs() < 1e-6, "y = {}", forward.y);
        assert!((forward.z + 1.0).abs() < 1e-6, "z = {}", forward.z);
    }

    #[test]
    fn looking_straight_up_keeps_the_direction_unit_length() {
        // Тот самый множитель `cos(pitch)` у горизонтальных составляющих:
        // без него при `pitch = 90` вектор был бы (cos(yaw), 1, sin(yaw)) —
        // длиной √2, и камера поехала бы тем сильнее, чем выше смотришь
        let mut scene = Scene::new();
        scene.pitch = 90.0;

        let forward = scene.forward();

        assert!((forward.y - 1.0).abs() < 1e-6, "смотрим не строго вверх");
        assert!(
            (forward.length() - 1.0).abs() < 1e-6,
            "длина {} вместо единицы",
            forward.length()
        );
    }

    /// Главное здесь — связка `forward` и `view_matrix`: точка ровно перед
    /// камерой обязана оказаться на оси −Z пространства вида, где бы сама
    /// камера ни стояла и куда бы ни смотрела.
    ///
    /// Это и есть тот инвариант, ради которого матрица вида вынесена в общее
    /// место: оба бэкенда строят её отсюда, и если формула разъедется, кадры
    /// разойдутся молча — оба останутся правдоподобными
    #[test]
    fn a_point_straight_ahead_lands_on_the_view_axis() {
        let mut scene = Scene::new();
        scene.camera_position = Vec3::new(3.0, -2.0, 7.0);
        scene.yaw = 33.0;
        scene.pitch = -18.0;

        let ahead = scene.camera_position + scene.forward();
        let in_view = &scene.view_matrix() * ahead;

        assert!((in_view.x).abs() < 1e-5, "съехало вбок: {}", in_view.x);
        assert!((in_view.y).abs() < 1e-5, "съехало вверх: {}", in_view.y);
        // Минус, а не плюс: пространство вида смотрит вдоль −Z — та же
        // конвенция, под которую посчитан `Mat4::perspective`
        assert!((in_view.z + 1.0).abs() < 1e-5, "по глубине: {}", in_view.z);
    }
}
