//! Свет, нормали и затенение: что попало в кадр и каким оттенком.

use std::collections::HashSet;

use xd_engine::{
    config::{AMBIENT_LIGHT, LIGHT_DIRECTION},
    math::Vec3,
    scene::{Instance, Mesh},
};

use crate::harness::{World, face_colors, render, tilted_cube};

#[test]
fn scene_with_a_cube_actually_draws_pixels() {
    let mut scene = World::new();
    let cube = tilted_cube(&mut scene, Vec3::new(0.0, 0.0, 0.0));
    scene.add_instance(cube);

    let frame = render(&scene);
    let painted = frame.chunks_exact(4).filter(|p| p[3] != 0).count();

    assert!(painted > 0, "кадр пустой");
}

#[test]
fn empty_scene_leaves_the_buffer_untouched() {
    let frame = render(&World::new());

    assert!(frame.iter().all(|byte| *byte == 0));
}

#[test]
fn flat_shading_gives_one_color_per_visible_face() {
    let mut scene = World::new();
    let cube = tilted_cube(&mut scene, Vec3::new(0.0, 0.0, 0.0));
    scene.add_instance(cube);

    // Три видимые грани куба, три разных наклона к свету, три оттенка.
    // Если бы затенение считалось не по граням, оттенков было бы больше
    assert_eq!(face_colors(&render(&scene)).len(), 3);
}

#[test]
fn shading_does_not_depend_on_world_position() {
    // Регрессия на нормали, но уже через весь пайплайн целиком.
    //
    // Двигаем куб и камеру на один и тот же вектор: взаимное расположение
    // не меняется, значит и освещение обязано остаться прежним. Пока нормаль
    // умножалась как точка, к ней прибавлялась позиция объекта — и этот тест
    // разошёлся бы
    let offset = Vec3::new(20.0, -13.0, 7.0);

    let mut at_origin = World::new();
    let cube = tilted_cube(&mut at_origin, Vec3::new(0.0, 0.0, 0.0));
    at_origin.add_instance(cube);

    let mut far_away = World::new();
    let cube = tilted_cube(&mut far_away, offset);
    far_away.add_instance(cube);
    far_away.scene.camera_position = at_origin.scene.camera_position + offset;

    assert_eq!(
        face_colors(&render(&at_origin)),
        face_colors(&render(&far_away))
    );
}

#[test]
fn face_turned_towards_the_light_is_brighter_than_one_turned_away() {
    // Свет светит из (0.5, 1.0, 0.8) — сверху. Значит верхняя грань куба
    // должна быть светлее нижней, а не наоборот
    let mut scene = World::new();

    let cube_mesh = scene.add_mesh(Mesh::create_cube());
    let mut cube =
        Instance::new(cube_mesh, Vec3::new(0.0, 0.0, 0.0)).with_color([255, 255, 255, 255]);
    // Смотрим на куб сверху, чтобы верхняя грань попала в кадр
    cube.rotation = Vec3::new(0.0, 0.0, 0.0);
    scene.add_instance(cube);
    scene.scene.camera_position = Vec3::new(0.0, 5.0, 0.0001);
    scene.scene.pitch = -89.0;
    scene.scene.yaw = -90.0;

    let from_above = face_colors(&render(&scene));

    // Тот же куб снизу
    scene.scene.camera_position = Vec3::new(0.0, -5.0, 0.0001);
    scene.scene.pitch = 89.0;

    let from_below = face_colors(&render(&scene));

    let brightest = |colors: &HashSet<[u8; 4]>| colors.iter().map(|c| c[0]).max().unwrap();

    assert!(
        brightest(&from_above) > brightest(&from_below),
        "сверху {:?}, снизу {:?}",
        brightest(&from_above),
        brightest(&from_below)
    );
}

#[test]
fn squashing_an_object_turns_its_normals_towards_the_light() {
    // Неравномерный масштаб — единственный случай, где нормаль нельзя гнать
    // по модельной матрице. Сплющим наклонную грань по Y: она станет ПОЛОЖЕ,
    // то есть её нормаль обязана повернуться К оси Y, а свет у нас как раз
    // сверху — значит грань станет ЯРЧЕ. Наивное умножение сплющит вместе с
    // гранью и саму нормаль, положит её набок, и грань потемнеет.
    //
    // Одна-единственная грань вместо готового меша нарочно: у куба нормали
    // идут по осям, и масштаб по осям меняет им только длину, а её съедает
    // normalize() — на кубе эта ошибка не видна вообще.
    const SQUASH: f32 = 0.3;

    let slope = |world: &mut World, scale_y: f32| {
        let mesh = world.add_mesh(Mesh::flat_shaded(
            &[
                Vec3::new(-1.0, -1.0, 0.0),
                Vec3::new(1.0, -1.0, 0.0),
                Vec3::new(0.0, 1.0, -1.0),
            ],
            &[[0, 1, 2]],
        ));

        let face = world.scene.spawn(mesh, Vec3::new(0.0, 0.0, 0.0));
        face.scale = Vec3::new(1.0, scale_y, 1.0);
        face.color = [255, 255, 255, 255];
    };

    let brightest = |frame: &[u8]| face_colors(frame).iter().map(|c| c[0]).max().unwrap();

    // Ожидание считается на бумаге, а не повторением кода. Нормаль грани по
    // построению — cross((2,0,0), (1,2,-1)) = (0, 2, 4). Честная нормаль
    // сплющенной грани получается делением на масштаб (это и есть обратная
    // транспонированная для диагональной матрицы), наивная — умножением
    let lit = |normal: Vec3| {
        let lambert = normal
            .normalize()
            .dot(&LIGHT_DIRECTION.normalize())
            .max(0.0);

        ((AMBIENT_LIGHT + (1.0 - AMBIENT_LIGHT) * lambert) * 255.0) as u8
    };

    let expected_plain = lit(Vec3::new(0.0, 2.0, 4.0)); // 225
    let expected_squashed = lit(Vec3::new(0.0, 2.0 / SQUASH, 4.0)); // 240
    let expected_naive = lit(Vec3::new(0.0, 2.0 * SQUASH, 4.0)); // 194

    let mut plain = World::new();
    slope(&mut plain, 1.0);

    let mut squashed = World::new();
    slope(&mut squashed, SQUASH);

    let got_plain = brightest(&render(&plain));
    let got_squashed = brightest(&render(&squashed));

    // Допуск в единицу, а не точное равенство: цвет вершины едет через
    // интерполяцию с делением на w, и хотя у всех трёх вершин он одинаковый,
    // последний бит мантиссы после деления совпадать не обязан
    assert!(
        got_plain.abs_diff(expected_plain) <= 1,
        "без масштаба: ожидали {expected_plain}, получили {got_plain}"
    );
    assert!(
        got_squashed.abs_diff(expected_squashed) <= 1,
        "сплющенная: ожидали {expected_squashed}, получили {got_squashed}"
    );

    // Главное утверждение: сплющенная грань СВЕТЛЕЕ. Наивный перенос нормали
    // дал бы {expected_naive} — темнее исходной, то есть тест покраснел бы
    assert!(
        got_squashed > got_plain,
        "сплющенная {got_squashed} не ярче исходной {got_plain}; \
         наивный перенос нормали дал бы {expected_naive}"
    );
}

/// Тот же самый шар, но нормали посчитаны по-плоски: позиции и треугольники
/// берём у гладкого меша, меняется ровно один атрибут
fn faceted_copy(smooth: &Mesh) -> Mesh {
    let positions: Vec<Vec3> = smooth.vertices.iter().map(|v| v.position).collect();

    Mesh::flat_shaded(&positions, &smooth.triangles)
}

fn sphere_scene(mesh: Mesh) -> World {
    let mut scene = World::new();

    let id = scene.add_mesh(mesh);
    let mut sphere = Instance::new(id, Vec3::new(0.0, 0.0, 0.0)).with_color([255, 255, 255, 255]);
    sphere.scale = Vec3::new(1.6, 1.6, 1.6);

    scene.add_instance(sphere);
    scene
}

#[test]
fn gouraud_turns_facets_into_a_gradient() {
    // Геометрия одна и та же, отличаются только нормали. У плоского варианта
    // все три вершины грани дают одну яркость и грань выходит однотонной;
    // у гладкого яркости в вершинах разные, и интерполяция размазывает их
    let smooth = Mesh::create_sphere(12, 16);
    let faceted = faceted_copy(&smooth);

    let smooth_shades = face_colors(&render(&sphere_scene(smooth))).len();
    let faceted_shades = face_colors(&render(&sphere_scene(faceted))).len();

    assert!(
        smooth_shades > faceted_shades * 3,
        "градиента не вышло: гладкий дал {} оттенков, гранёный {}",
        smooth_shades,
        faceted_shades
    );
}

#[test]
fn faceted_mesh_has_no_more_shades_than_triangles() {
    // Проверка от обратного: при плоском затенении число оттенков ограничено
    // числом граней, потому что внутри грани интерполировать нечего
    let faceted = faceted_copy(&Mesh::create_sphere(12, 16));
    let triangle_count = faceted.triangles.len();

    assert!(face_colors(&render(&sphere_scene(faceted))).len() <= triangle_count);
}

#[test]
fn smooth_normals_stay_unit_length() {
    // smooth_shaded складывает ненормализованные векторные произведения (длина
    // такого вектора равна удвоенной площади грани, поэтому крупные грани
    // весят больше) и нормализует только сумму — длина обязана получиться
    // единичной. Ненормированная нормаль тихо испортила бы яркость по Ламберту.
    //
    // Меш тут собственный, а не сфера: сфера с некоторых пор берёт нормали из
    // формулы, а не усреднением, и этот путь больше не проверяет
    let positions = [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(2.0, 0.0, 0.0),
        Vec3::new(2.0, 2.0, 0.0),
        Vec3::new(0.0, 2.0, 0.0),
    ];

    let mesh = Mesh::smooth_shaded(&positions, &[[0, 1, 2], [0, 2, 3]]);

    for vertex in &mesh.vertices {
        assert!(
            (vertex.normal.length() - 1.0).abs() < 1e-4,
            "нормаль длины {}",
            vertex.normal.length()
        );
    }
}
