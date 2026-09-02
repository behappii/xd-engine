//! Текстурирование: как тексель ложится на посчитанный свет.

use xd_engine::{
    math::Vec3,
    scene::{Instance, Mesh, TextureId},
    texture::{Magnify, Minify, Texture},
};

use crate::harness::{HEIGHT, WIDTH, World, face_colors, render, tilted_cube};

/// Шахматка 2×2 клетки: белая и красная.
///
/// Клетки различаются оттенком, а не яркостью, и это принципиально для
/// точного счёта цветов. Умножение на белый тексель — это ×1, оно возвращает
/// ровно тот байт, что и без текстуры; умножение на чистый красный обнуляет
/// два канала и оставляет тот же байт в третьем. Никаких новых округлений
/// не появляется, поэтому набор цветов предсказуем побайтово.
///
/// Первая версия теста брала тёмно-серую клетку и ломалась: неосвещённая
/// грань светится ровно фоновой подсветкой AMBIENT_LIGHT = 0.25, а 0.25 от
/// 60/255 — это точно 15/255, то есть граница между двумя байтами. Половина
/// пикселей грани падала по одну сторону, половина по другую, и «6 оттенков»
/// превращались в 7
fn checker(world: &mut World) -> TextureId {
    world.add_texture(Texture::checker(
        8,
        2,
        [255, 255, 255, 255],
        [255, 0, 0, 255],
    ))
}

#[test]
fn texture_multiplies_the_number_of_shades_by_the_number_of_texels() {
    // Развёртка куба отдаёт каждой грани весь квадрат текстуры, поэтому на
    // каждой из трёх видимых граней встречаются обе клетки шахматки. Свет
    // текстуру не заменяет, а модулирует — значит оттенков должно стать
    // ровно вдвое больше: 3 грани × 2 клетки.
    //
    // Тест ловит сразу две ошибки. Если UV не доехало до растеризатора, вся
    // грань прочитает один тексель и оттенков останется 3 (столько же, сколько
    // насчитал flat_shading_gives_one_color_per_visible_face). Если текстура
    // затирает свет вместо умножения, оттенков станет 2 — по числу клеток
    let mut scene = World::new();
    let texture = checker(&mut scene);
    let cube = tilted_cube(&mut scene, Vec3::new(0.0, 0.0, 0.0)).with_texture(texture);
    scene.add_instance(cube);

    assert_eq!(face_colors(&render(&scene)).len(), 6);
}

#[test]
fn a_white_texture_changes_nothing() {
    // Умножение на единицу обязано быть тождественным. Тест сторожит именно
    // то, что текстурная ветка не делает с цветом ничего лишнего — например,
    // не теряет перспективную коррекцию и не подмешивает собственную яркость
    let mut plain = World::new();
    let cube = tilted_cube(&mut plain, Vec3::new(0.0, 0.0, 0.0));
    plain.add_instance(cube);

    let mut textured = World::new();
    let white = textured.add_texture(Texture::checker(
        4,
        1,
        [255, 255, 255, 255],
        [255, 255, 255, 255],
    ));
    let cube = tilted_cube(&mut textured, Vec3::new(0.0, 0.0, 0.0)).with_texture(white);
    textured.add_instance(cube);

    // Побайтовое совпадение всего кадра, а не только набора цветов
    assert_eq!(render(&plain), render(&textured));
}

#[test]
fn an_instance_without_a_texture_is_unaffected_by_its_neighbour() {
    // Текстура — состояние, которое сцена переключает между инстансами.
    // Забыть сбросить его — классическая ошибка «протёкшего» стейта:
    // следующий объект отрисовался бы чужой картинкой
    let mut alone = World::new();
    let cube = tilted_cube(&mut alone, Vec3::new(0.0, 0.0, 0.0));
    alone.add_instance(cube);

    let mut after_textured = World::new();
    // Текстурированный куб стоит далеко в стороне и в кадр не попадает —
    // важно только то, что он обрабатывается раньше
    let texture = checker(&mut after_textured);
    let far = tilted_cube(&mut after_textured, Vec3::new(-40.0, 0.0, 0.0)).with_texture(texture);
    after_textured.add_instance(far);
    let near = tilted_cube(&mut after_textured, Vec3::new(0.0, 0.0, 0.0));
    after_textured.add_instance(near);

    assert_eq!(
        face_colors(&render(&alone)),
        face_colors(&render(&after_textured))
    );
}

#[test]
fn uv_beyond_one_tiles_the_texture() {
    // Развёртка куба лежит в 0..1, но UV можно масштабировать — тогда режим
    // repeat раскладывает картинку плиткой. Проверяем, что координаты за
    // пределами квадрата не обрезаются в край и не паникуют: у растянутой
    // вчетверо шахматки клеток на грани должно стать заметно больше,
    // а набор цветов — остаться прежним
    let mut mesh = Mesh::create_cube();

    for vertex in &mut mesh.vertices {
        vertex.uv = vertex.uv * 4.0;
    }

    let mut tiled = World::new();
    let tiled_mesh = tiled.add_mesh(mesh);
    let tiled_texture = checker(&mut tiled);
    let mut instance = Instance::new(tiled_mesh, Vec3::new(0.0, 0.0, 0.0))
        .with_color([255, 255, 255, 255])
        .with_texture(tiled_texture);
    instance.rotation = Vec3::new(20.0, 35.0, 0.0);
    tiled.add_instance(instance);

    let mut plain = World::new();
    let plain_texture = checker(&mut plain);
    let cube = tilted_cube(&mut plain, Vec3::new(0.0, 0.0, 0.0)).with_texture(plain_texture);
    plain.add_instance(cube);

    // Цвета те же самые — меняется только их раскладка по поверхности
    assert_eq!(face_colors(&render(&tiled)), face_colors(&render(&plain)));

    // А вот переходов между клетками вдоль строки стало больше
    assert!(
        colour_switches_in_row(&render(&tiled), 75) > colour_switches_in_row(&render(&plain), 75)
    );
}

/// Сколько раз цвет меняется вдоль строки кадра. Грубая мера «дробности»
/// узора: чем мельче клетки, тем больше переходов
fn colour_switches_in_row(frame: &[u8], y: u32) -> usize {
    let row_start = (y * WIDTH * 4) as usize;
    let row = &frame[row_start..row_start + (WIDTH * 4) as usize];

    row.chunks_exact(4)
        .zip(row.chunks_exact(4).skip(1))
        .filter(|(a, b)| a != b)
        .count()
}

/// Пол с мелкой плиткой, уходящий к горизонту, и камера низко над ним
fn horizon_floor(magnify: Magnify, minify: Minify) -> Vec<u8> {
    let mut world = World::new();

    // Плитка мелкая: UV домножается на 40, значит на полу 40x40 копий
    // картинки. У горизонта в один пиксель попадает целая пачка текселей
    let mut mesh = Mesh::create_cube();
    for vertex in &mut mesh.vertices {
        vertex.uv = vertex.uv * 40.0;
    }

    let mesh = world.add_mesh(mesh);
    let texture = world.add_texture(
        Texture::checker(8, 2, [255, 255, 255, 255], [0, 0, 0, 255]).with_filter(magnify, minify),
    );

    let floor = world.scene.spawn(mesh, Vec3::new(0.0, -1.0, 0.0));
    floor.scale = Vec3::new(60.0, 0.1, 60.0);
    floor.color = [255, 255, 255, 255];
    floor.texture = Some(texture);

    // Камера чуть выше пола и смотрит почти горизонтально: так плитка уходит
    // до самого горизонта, и отпечаток пикселя растёт по всей высоте кадра
    world.scene.camera_position = Vec3::new(0.0, -0.6, 0.0);
    world.scene.pitch = -2.0;

    render(&world)
}

/// Полная вариация строки: сумма модулей скачков яркости между соседями.
///
/// Это и есть мера ряби. Числом ПЕРЕХОДОВ её не измерить: плавный градиент
/// меняется в каждом пикселе и даёт переходов даже больше, чем рябь, — просто
/// каждый крошечный. Важна не частота изменений, а их размах
fn total_variation_in_row(frame: &[u8], y: u32) -> u32 {
    let row_start = (y * WIDTH * 4) as usize;
    let row = &frame[row_start..row_start + (WIDTH * 4) as usize];

    row.chunks_exact(4)
        .zip(row.chunks_exact(4).skip(1))
        .map(|(a, b)| a[0].abs_diff(b[0]) as u32)
        .sum()
}

#[test]
fn mipmaps_calm_down_the_floor_at_the_horizon() {
    // Рябь у горизонта — не дефект выборки, а следствие того, что текселей
    // в пикселе больше одного. Ближайший сосед берёт из пачки один, и при
    // малейшем сдвиге камеры выбранный меняется: картинка кипит.
    //
    // Мип-уровни это и лечат. Меряем полной вариацией строки, и обе половины
    // важны: вдали рябь обязана пропасть, а ВБЛИЗИ картинка обязана остаться
    // такой же резкой. Без второй половины тест прошёл бы и от «всегда брать
    // самый грубый уровень» — то есть от честного размытия всего кадра.
    //
    // Заодно это единственный тест, который вообще проверяет производные UV
    // в растеризаторе: сама текстура их только принимает, а считает
    // `Gradients` в renderer/triangle.rs, и ошибка там видна ровно здесь
    let sharp = horizon_floor(Magnify::Nearest, Minify::Nearest);
    let smooth = horizon_floor(Magnify::Linear, Minify::Mipmapped);
    // Сочетание, ради которого настройки разведены на две, и то самое, что
    // стоит в демо-сцене: вблизи ближайший сосед, вдали мип-уровни
    let mixed = horizon_floor(Magnify::Nearest, Minify::Mipmapped);

    // Горизонт при pitch = -2 стоит чуть выше середины кадра, так что
    // середина — это уже далёкий пол, а на двадцать строк ниже — ближний
    let far = HEIGHT / 2;
    let near = HEIGHT / 2 + 20;

    let (sharp_far, smooth_far) = (
        total_variation_in_row(&sharp, far),
        total_variation_in_row(&smooth, far),
    );
    let (sharp_near, smooth_near) = (
        total_variation_in_row(&sharp, near),
        total_variation_in_row(&smooth, near),
    );

    assert!(
        sharp_far > 0,
        "пол не попал в строку {far} — проверять нечего"
    );

    assert!(
        smooth_far * 10 < sharp_far,
        "рябь вдали осталась: {smooth_far} против {sharp_far}"
    );

    assert!(
        smooth_near * 10 >= sharp_near * 8,
        "вблизи картинку размыло зря: {smooth_near} против {sharp_near}"
    );

    // И главное — обе половины достижимы ОДНОВРЕМЕННО. Вдали `mixed` обязан
    // вести себя как трилинейный (сжатие у них одинаковое), а вблизи —
    // побайтово как ближайший сосед, потому что растяжение у них тоже
    // одинаковое. Именно этого сочетания и не было, пока настройка была одна
    assert!(
        total_variation_in_row(&mixed, far) * 10 < sharp_far,
        "рябь вдали осталась и у смешанной настройки"
    );
    assert_eq!(
        total_variation_in_row(&mixed, near),
        sharp_near,
        "вблизи смешанная настройка обязана совпасть с ближайшим соседом"
    );
}
