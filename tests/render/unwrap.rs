//! Развёртки: куда именно на геометрию ложится картинка.

use xd_engine::{
    math::Vec3,
    scene::{Instance, Mesh},
    texture::Texture,
};

use crate::harness::{WIDTH, World, painted_bounds, render};

#[test]
fn the_cube_unwrap_puts_the_texture_upright_on_the_face() {
    // Четвёрки углов в `create_cube` выведены руками: обход против часовой
    // стрелки снаружи — чтобы нормаль смотрела наружу, начало с левого нижнего
    // угла — чтобы картинка стояла ровно. Первое условие ловят тесты нормалей,
    // а второе не поймает ничего: перепутанный стартовый угол просто повернёт
    // текстуру, и на шахматке это вообще не видно.
    //
    // Поэтому текстура здесь несимметричная: четыре разных цвета по четвертям.
    // Камера смотрит на грань +Z в упор, и каждая четверть картинки обязана
    // оказаться в своей четверти экрана
    let texture = Texture::from_rgba8(
        2,
        2,
        &[
            255, 0, 0, 255, // верх-лево  — красный
            0, 255, 0, 255, // верх-право — зелёный
            0, 0, 255, 255, // низ-лево   — синий
            255, 255, 255, 255, // низ-право  — белый
        ],
    );

    let mut scene = World::new();
    let mesh = scene.add_mesh(Mesh::create_cube());
    let texture = scene.add_texture(texture);
    scene.add_instance(
        Instance::new(mesh, Vec3::new(0.0, 0.0, 0.0))
            .with_color([255, 255, 255, 255])
            .with_texture(texture),
    );

    let frame = render(&scene);

    // Грань занимает середину кадра; отступаем от центра на четверть,
    // чтобы гарантированно попасть внутрь нужной четверти
    let pixel = |x: u32, y: u32| {
        let i = ((y * WIDTH + x) * 4) as usize;
        [frame[i], frame[i + 1], frame[i + 2]]
    };

    // Какой канал ярче — тот цвет и лежит в этом пикселе
    let dominant = |p: [u8; 3]| {
        if p[0] > 0 && p[1] == 0 && p[2] == 0 {
            "красный"
        } else if p[1] > 0 && p[0] == 0 && p[2] == 0 {
            "зелёный"
        } else if p[2] > 0 && p[0] == 0 && p[1] == 0 {
            "синий"
        } else if p[0] > 0 && p[1] > 0 && p[2] > 0 {
            "белый"
        } else {
            "пусто"
        }
    };

    // Точки берём по четвертям РЕАЛЬНОГО габарита грани, а не по четвертям
    // кадра: куб занимает лишь его середину, и фиксированный отступ от центра
    // легко промахнулся бы мимо фигуры в фон
    let (min_x, min_y, max_x, max_y) =
        painted_bounds(&frame, WIDTH).expect("куб не попал в кадр, проверять нечего");

    let left = min_x + (max_x - min_x) / 4;
    let right = max_x - (max_x - min_x) / 4;
    let top = min_y + (max_y - min_y) / 4;
    let bottom = max_y - (max_y - min_y) / 4;

    assert_eq!(dominant(pixel(left, top)), "красный", "верх-лево");
    assert_eq!(dominant(pixel(right, top)), "зелёный", "верх-право");
    assert_eq!(dominant(pixel(left, bottom)), "синий", "низ-лево");
    assert_eq!(dominant(pixel(right, bottom)), "белый", "низ-право");
}

#[test]
fn sphere_splits_the_seam_so_the_texture_can_close() {
    let (stacks, slices) = (12, 16);
    let sphere = Mesh::create_sphere(stacks, slices);

    // Сетка с лишним столбцом (шов) и лишней строкой (полюса). Раньше сфера
    // замыкала кольцо остатком от деления, и вершин было 2 + 11 * 16
    assert_eq!(sphere.vertices.len(), (stacks + 1) * (slices + 1));

    // Концы кольца посередине сферы: одна и та же точка пространства, но на
    // картинке — левый и правый края развёртки
    let row = (stacks / 2) * (slices + 1);
    let left = sphere.vertices[row];
    let right = sphere.vertices[row + slices];

    assert!(
        (left.position - right.position).length() < 1e-5,
        "концы кольца разъехались: {:?} и {:?}",
        left.position,
        right.position
    );
    assert_eq!(left.uv.x, 0.0);
    assert_eq!(right.uv.x, 1.0);

    // Главное условие, из-за которого развёртки у сферы долго не было: у
    // близнецов на шве обязана быть ОДНА И ТА ЖЕ нормаль. Раньше нормали
    // считались усреднением по сходящимся граням, и каждой копии досталась бы
    // половина соседей — по сфере пошла бы видимая полоса
    assert!(
        (left.normal - right.normal).length() < 1e-5,
        "нормали на шве разошлись: {:?} и {:?}",
        left.normal,
        right.normal
    );
}

#[test]
fn sphere_normals_are_the_positions_themselves() {
    // У сферы единичного радиуса нормаль в точке известна точно и совпадает с
    // самой позицией. Усреднение по граням это лишь приближает — и именно оно
    // мешало дублировать вершины на шве
    for vertex in &Mesh::create_sphere(8, 12).vertices {
        assert!(
            (vertex.normal - vertex.position).length() < 1e-5,
            "нормаль {:?} не совпала с позицией {:?}",
            vertex.normal,
            vertex.position
        );
    }
}

#[test]
fn the_pole_cap_takes_the_middle_of_its_slice_not_the_edge() {
    // У полюса четырёхугольник вырожден в треугольник, и его вершине годится
    // любое u внутри доли — геометрия от этого не меняется. Но край доли даёт
    // в текстуре прямоугольный треугольник вместо равнобедренного, и картинка
    // у полюсов заметно косит. Ошибка совершенно тихая: ни нормали, ни обход,
    // ни силуэт от неё не страдают, поймать её может только эта проверка
    let sphere = Mesh::create_sphere(4, 8);

    let [apex, right, left] = sphere.triangles[0];
    let apex = sphere.vertices[apex];
    let (left, right) = (sphere.vertices[left], sphere.vertices[right]);

    assert!(
        (apex.position.y - 1.0).abs() < 1e-5,
        "первый треугольник должен быть северной шапкой"
    );

    let middle = (left.uv.x + right.uv.x) / 2.0;

    assert!(
        (apex.uv.x - middle).abs() < 1e-5,
        "вершина шапки получила u = {}, а середина доли — {middle}",
        apex.uv.x
    );
}

#[test]
fn the_sphere_unwrap_puts_the_top_of_the_texture_on_the_north_pole() {
    // Текстура из двух строк: верхняя красная, нижняя синяя. v = 0 — верхняя
    // строка картинки, северный полюс сферы — тоже v = 0, значит красное
    // обязано лечь сверху. Ширина 1 — чтобы u ни на что не влиял и тест
    // проверял ровно одно.
    //
    // Перевёрнутая по вертикали развёртка — ошибка, которой на однотонной или
    // симметричной текстуре не видно вовсе
    let texture = Texture::from_rgba8(1, 2, &[255, 0, 0, 255, /**/ 0, 0, 255, 255]);

    let mut scene = World::new();
    let mesh = scene.add_mesh(Mesh::create_sphere(16, 24));
    let texture = scene.add_texture(texture);
    let mut sphere = Instance::new(mesh, Vec3::new(0.0, 0.0, 0.0))
        .with_color([255, 255, 255, 255])
        .with_texture(texture);
    sphere.scale = Vec3::new(1.6, 1.6, 1.6);
    scene.add_instance(sphere);

    let frame = render(&scene);
    let (min_x, min_y, max_x, max_y) =
        painted_bounds(&frame, WIDTH).expect("сфера не нарисовалась");

    let pixel = |x: u32, y: u32| {
        let i = ((y * WIDTH + x) * 4) as usize;
        [frame[i], frame[i + 1], frame[i + 2]]
    };

    // Отступаем от края силуэта на шестую часть высоты: там уже видна лицевая
    // поверхность, а не касательный к камере край
    let step = (max_y - min_y) / 6;
    let center_x = (min_x + max_x) / 2;

    let top = pixel(center_x, min_y + step);
    let bottom = pixel(center_x, max_y - step);

    assert!(
        top[0] > top[2],
        "у северного полюса ждали красное, вышло {top:?}"
    );
    assert!(
        bottom[2] > bottom[0],
        "у южного полюса ждали синее, вышло {bottom:?}"
    );
}
