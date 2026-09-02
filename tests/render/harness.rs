//! Общее хозяйство интеграционных тестов: сцена вместе с ресурсами, рендер
//! в буфер и разбор получившихся пикселей.
//!
//! Лежит отдельно, потому что нужно почти всем файлам рядом. Всё, чем
//! пользуется только один из них, живёт в нём же, а не здесь

use std::collections::HashSet;

use xd_engine::{
    math::Vec3,
    scene::{Assets, Instance, Mesh, MeshId, Scene, TextureId},
    texture::Texture,
};

/// Сцена вместе со своими ресурсами.
///
/// Ресурсы и мир разделены в движке нарочно — арены переживают любую отдельную
/// сцену. Но в тестах они всегда ходят парой, поэтому держим их рядом, чтобы
/// каждый тест не таскал два значения и не забывал их сопоставлять
pub struct World {
    pub assets: Assets,
    pub scene: Scene,
}

impl World {
    pub fn new() -> Self {
        Self {
            assets: Assets::new(),
            scene: Scene::new(),
        }
    }

    pub fn add_mesh(&mut self, mesh: Mesh) -> MeshId {
        self.assets.add_mesh(mesh)
    }

    pub fn add_texture(&mut self, texture: Texture) -> TextureId {
        self.assets.add_texture(texture)
    }

    pub fn add_instance(&mut self, instance: Instance) {
        self.scene.add_instance(instance);
    }
}

pub const WIDTH: u32 = 200;
pub const HEIGHT: u32 = 150;

pub fn render_at(world: &World, width: u32, height: u32) -> Vec<u8> {
    let mut frame = vec![0u8; (width * height * 4) as usize];
    let mut depth = vec![0.0f32; (width * height) as usize];

    world
        .scene
        .draw(&world.assets, &mut frame, &mut depth, width, height);

    frame
}

pub fn render(world: &World) -> Vec<u8> {
    render_at(world, WIDTH, HEIGHT)
}

/// Цвета всех закрашенных пикселей. Фон — нули, его отбрасываем.
/// При плоском затенении каждая видимая грань даёт ровно один цвет,
/// так что размер множества — это число видимых граней
pub fn face_colors(frame: &[u8]) -> HashSet<[u8; 4]> {
    frame
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .filter(|p| *p != [0, 0, 0, 0])
        .collect()
}

/// Куб, развёрнутый так, чтобы камера видела сразу три грани.
///
/// Меш регистрируется в сцене прямо здесь: инстанс хранит только MeshId,
/// и без своей сцены он ничего не значит
pub fn tilted_cube(world: &mut World, position: Vec3) -> Instance {
    let mesh = world.add_mesh(Mesh::create_cube());
    let mut cube = Instance::new(mesh, position).with_color([255, 255, 255, 255]);

    cube.rotation = Vec3::new(20.0, 35.0, 0.0);

    cube
}

/// Габаритный прямоугольник закрашенной области: (min_x, min_y, max_x, max_y),
/// границы включительно. `None`, если не закрашено ничего
pub fn painted_bounds(frame: &[u8], width: u32) -> Option<(u32, u32, u32, u32)> {
    let painted: Vec<(u32, u32)> = frame
        .chunks_exact(4)
        .enumerate()
        .filter(|(_, p)| p[3] != 0)
        .map(|(i, _)| (i as u32 % width, i as u32 / width))
        .collect();

    if painted.is_empty() {
        return None;
    }

    let xs = painted.iter().map(|(x, _)| *x);
    let ys = painted.iter().map(|(_, y)| *y);

    Some((
        xs.clone().min().unwrap(),
        ys.clone().min().unwrap(),
        xs.max().unwrap(),
        ys.max().unwrap(),
    ))
}

/// Размер закрашенной области в пикселях: (ширина, высота) габаритного
/// прямоугольника. Ноль на ноль, если не закрашено ничего
pub fn painted_size(frame: &[u8], width: u32) -> (u32, u32) {
    match painted_bounds(frame, width) {
        // +1, потому что габарит из одного пикселя — это ширина 1, а не 0
        Some((min_x, min_y, max_x, max_y)) => (max_x - min_x + 1, max_y - min_y + 1),
        None => (0, 0),
    }
}
