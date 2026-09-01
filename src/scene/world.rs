use crate::math::Vec3;

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
