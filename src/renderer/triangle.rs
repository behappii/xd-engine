use crate::{
    config::EPSILON,
    math::{Vec2, Vec4},
};

use super::{DrawContext, ShadedVertex, screen::ndc_to_screen_f};

/// Знаковая площадь параллелограмма на векторах (a->b) и (a->p)
#[inline]
fn edge(ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32) -> f32 {
    (bx - ax) * (py - ay) - (by - ay) * (px - ax)
}

/// Насколько быстро интерполируемые величины меняются при шаге на пиксель.
///
/// Нужно мип-уровням: чтобы выбрать уровень пирамиды, надо знать отпечаток
/// пикселя в текселях, то есть производные UV по экрану.
///
/// Хранятся производные НЕ самого UV, а тех двух величин, которые линейны по
/// экрану: `uv/w` и `1/w`. У них производные постоянны на весь треугольник —
/// поэтому и считаются один раз здесь, а не в цикле по пикселям. Само UV
/// линейным не является, и его производная в каждом пикселе своя;
/// [`Gradients::uv_derivatives`] достаёт её из этих четырёх констант
struct Gradients {
    uv_over_w_dx: Vec2,
    uv_over_w_dy: Vec2,
    inv_w_dx: f32,
    inv_w_dy: f32,
}

impl Gradients {
    /// `uv_over_w` и `inv_w` — уже поделённые на w атрибуты вершин,
    /// `bary_scale` — общий множитель, превращающий знаковую площадь
    /// в барицентрическую координату
    fn of_triangle(
        p0: [f32; 2],
        p1: [f32; 2],
        p2: [f32; 2],
        uv_over_w: [Vec2; 3],
        inv_w: [f32; 3],
        bary_scale: f32,
    ) -> Self {
        let ([x0, y0], [x1, y1], [x2, y2]) = (p0, p1, p2);

        // Барицентрика b_i = edge_i * bary_scale, а edge — многочлен первой
        // степени по (x, y), поэтому её производные — просто коэффициенты:
        // у edge(a, b, p) производная по x равна -(b.y - a.y), по y равна
        // (b.x - a.x). Дальше остаётся домножить на общий масштаб
        let db_dx = [
            -(y2 - y1) * bary_scale,
            -(y0 - y2) * bary_scale,
            -(y1 - y0) * bary_scale,
        ];
        let db_dy = [
            (x2 - x1) * bary_scale,
            (x0 - x2) * bary_scale,
            (x1 - x0) * bary_scale,
        ];

        let combine_uv =
            |db: [f32; 3]| uv_over_w[0] * db[0] + uv_over_w[1] * db[1] + uv_over_w[2] * db[2];
        let combine_w = |db: [f32; 3]| inv_w[0] * db[0] + inv_w[1] * db[1] + inv_w[2] * db[2];

        Self {
            uv_over_w_dx: combine_uv(db_dx),
            uv_over_w_dy: combine_uv(db_dy),
            inv_w_dx: combine_w(db_dx),
            inv_w_dy: combine_w(db_dy),
        }
    }

    /// Производные самого UV в конкретном пикселе.
    ///
    /// Вывод. По экрану линейны `U = uv/w` и `W = 1/w`, а нужное нам
    /// `uv = U / W`. Производная частного:
    ///
    /// ```text
    /// d(U/W)/dx = (U' · W − U · W') / W²  =  (U' − uv · W') / W  =  (U' − uv · W') · w
    /// ```
    ///
    /// То есть хватает уже посчитанных в этом пикселе `uv` и `w` плюс двух
    /// констант треугольника — ни второй интерполяции, ни соседних пикселей
    /// не нужно. Настоящие видеокарты, кстати, идут другим путём: они считают
    /// разность внутри квадрата 2×2 пикселя, и именно поэтому производные
    /// доступны только во фрагментном шейдере
    #[inline]
    fn uv_derivatives(&self, uv: Vec2, w: f32) -> (Vec2, Vec2) {
        (
            (self.uv_over_w_dx - uv * self.inv_w_dx) * w,
            (self.uv_over_w_dy - uv * self.inv_w_dy) * w,
        )
    }
}

pub fn draw_triangle_filled(
    v0: ShadedVertex,
    v1: ShadedVertex,
    v2: ShadedVertex,
    ctx: &mut DrawContext,
) {
    let (p0, p1, p2) = (v0.clip_position, v1.clip_position, v2.clip_position);

    // Клиппинг гарантировал w > 0
    let iw0 = 1.0 / p0.w;
    let iw1 = 1.0 / p1.w;
    let iw2 = 1.0 / p2.w;

    // Атрибуты заранее делим на w.
    //
    // Барицентрические координаты мы считаем на ЭКРАНЕ, уже после
    // перспективного деления, а оно нелинейно: равные шаги по экрану
    // соответствуют разным шагам по поверхности треугольника. Линейно по
    // экрану меняется не сам атрибут, а его отношение к w — как и 1/w.
    // Поэтому интерполируем attr/w и 1/w по отдельности, а в пикселе делим
    // одно на другое и получаем attr. Без этого деления цвета и текстуры
    // «плывут» на гранях, повёрнутых к камере под углом
    let c0 = v0.color * iw0;
    let c1 = v1.color * iw1;
    let c2 = v2.color * iw2;

    // UV — обычный интерполируемый атрибут и проходит ровно ту же схему.
    // Именно на текстуре пропущенная поправка видна лучше всего: цвет «плывёт»
    // незаметно, а прямые линии шахматки на наклонной грани выгибаются дугой
    let uv0 = v0.uv * iw0;
    let uv1 = v1.uv * iw1;
    let uv2 = v2.uv * iw2;

    // Копия, а не обращение к ctx внутри цикла: ниже ctx одалживается
    // изменяемо ради записи пикселя, а Option<&Texture> — Copy
    let texture = ctx.texture;

    let (x0, y0) = ndc_to_screen_f(p0.x * iw0, p0.y * iw0, ctx.width, ctx.height);
    let (x1, y1) = ndc_to_screen_f(p1.x * iw1, p1.y * iw1, ctx.width, ctx.height);
    let (x2, y2) = ndc_to_screen_f(p2.x * iw2, p2.y * iw2, ctx.width, ctx.height);

    let area = edge(x0, y0, x1, y1, x2, y2);

    // Вырожденный треугольник (грань ровно ребром к камере)
    if area.abs() < EPSILON {
        return;
    }

    // Знак зависит от обхода; ndc_to_screen переворачивает Y, поэтому
    // не гадаем, а нормируем по фактическому знаку
    let sign = if area > 0.0 { 1.0 } else { -1.0 };
    let inv_area = 1.0 / area.abs();

    // Мип-уровню нужно знать, насколько быстро UV бежит по экрану. Считаем это
    // только если текстура спросит: производные стоят нескольких умножений на
    // КАЖДЫЙ закрашенный пиксель, и платить за них зря не хочется
    let gradients = texture
        .filter(|texture| texture.needs_derivatives())
        .map(|_| {
            Gradients::of_triangle(
                [x0, y0],
                [x1, y1],
                [x2, y2],
                [uv0, uv1, uv2],
                [iw0, iw1, iw2],
                sign * inv_area,
            )
        });

    // Габаритный прямоугольник, обрезанный по экрану —
    // здесь же бесплатно происходит отсечение по краям
    let min_x = x0.min(x1).min(x2).floor().max(0.0) as usize;
    let max_x = (x0.max(x1).max(x2).ceil() as i64).min(ctx.width as i64 - 1);
    let min_y = y0.min(y1).min(y2).floor().max(0.0) as usize;
    let max_y = (y0.max(y1).max(y2).ceil() as i64).min(ctx.height as i64 - 1);

    if max_x < 0 || max_y < 0 {
        return;
    }
    let (max_x, max_y) = (max_x as usize, max_y as usize);

    // Обрезка по полосе. При однопоточной отрисовке полоса — это весь кадр и
    // строки ниже ничего не меняют; при многопоточной каждый поток так
    // отбрасывает ту часть треугольника, которая лежит в чужой полосе.
    // Треугольник обходят ВСЕ потоки, но пиксели каждого пишет ровно один —
    // поэтому синхронизация не нужна вообще, а картинка получается той же
    // и в том же порядке, что и без потоков
    let min_y = min_y.max(ctx.y_offset as usize);
    let max_y = max_y.min((ctx.y_offset + ctx.rows) as usize - 1);

    if min_y > max_y {
        return;
    }

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            // Центр пикселя, а не угол
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;

            let e0 = edge(x1, y1, x2, y2, px, py) * sign;
            let e1 = edge(x2, y2, x0, y0, px, py) * sign;
            let e2 = edge(x0, y0, x1, y1, px, py) * sign;

            if e0 < 0.0 || e1 < 0.0 || e2 < 0.0 {
                continue;
            }

            // Барицентрические координаты
            let b0 = e0 * inv_area;
            let b1 = e1 * inv_area;
            let b2 = e2 * inv_area;

            // 1/w интерполируется в экранном пространстве линейно —
            // в отличие от самого z, который так интерполировать нельзя
            let inv_w = b0 * iw0 + b1 * iw1 + b2 * iw2;

            // Одно деление на всю пару атрибутов: возвращаемся от attr/w к attr
            let w = 1.0 / inv_w;

            // Тот же приём для цвета: интерполируем color/w, потом делим на
            // интерполированное 1/w — деление возвращает нас к самому цвету
            let color_over_w = c0 * b0 + c1 * b1 + c2 * b2;
            let color = color_over_w * w;

            // Свет модулирует текстуру, а не заменяет её: тексель умножается
            // на посчитанную в вершинах яркость покомпонентно. Поэтому
            // затенение по Гуро никуда не девается — оно просто ложится
            // поверх картинки, и грани по-прежнему темнеют с наклоном
            let color = match texture {
                Some(texture) => {
                    let uv = (uv0 * b0 + uv1 * b1 + uv2 * b2) * w;

                    let texel = match &gradients {
                        // Есть мип-уровни — надо знать отпечаток пикселя
                        Some(gradients) => {
                            let (duv_dx, duv_dy) = gradients.uv_derivatives(uv, w);

                            texture.sample_grad(uv, duv_dx, duv_dy)
                        }
                        None => texture.sample(uv),
                    };

                    color * texel
                }
                None => color,
            };

            ctx.set_pixel_depth(x, y, inv_w, color);
        }
    }
}

/// Грань повёрнута от камеры?
/// Соглашение: обход против часовой стрелки при взгляде снаружи.
pub fn is_backface(v0: Vec4, v1: Vec4, v2: Vec4) -> bool {
    // Если вершина за камерой, деление на w бессмысленно —
    // отдаём треугольник клипперу как есть
    if v0.w <= EPSILON || v1.w <= EPSILON || v2.w <= EPSILON {
        return false;
    }

    // Перспективное деление: clip space -> NDC
    let x0 = v0.x / v0.w;
    let y0 = v0.y / v0.w;
    let x1 = v1.x / v1.w;
    let y1 = v1.y / v1.w;
    let x2 = v2.x / v2.w;
    let y2 = v2.y / v2.w;

    // Знаковая площадь: положительная = обход против часовой = грань к нам
    let area = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0);

    area <= 0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        math::{Vec2, Vec3},
        texture::Texture,
    };

    const WIDTH: u32 = 100;
    const HEIGHT: u32 = 100;

    /// Вершина по координатам NDC: в clip space они умножены на w,
    /// поэтому экранное положение от выбора w не зависит — меняется
    /// только «глубина», а с ней и поправка при интерполяции
    fn vertex(ndc_x: f32, ndc_y: f32, w: f32, red: f32) -> ShadedVertex {
        ShadedVertex::new(
            Vec4 {
                x: ndc_x * w,
                y: ndc_y * w,
                z: 0.0,
                w,
            },
            Vec3::new(red, 0.0, 0.0),
        )
    }

    /// Треугольник с красной ближней вершиной слева (экранный x = 10) и двумя
    /// чёрными справа на одной вертикали (экранный x = 90).
    ///
    /// Две правые вершины стоят на общей вертикали не случайно: тогда вес
    /// левой вершины зависит только от x пикселя, b0 = (90 - x) / 80, и
    /// ожидаемый цвет можно посчитать на бумаге, не повторяя растеризатор
    fn render_gradient(w_far: f32) -> Vec<u8> {
        let mut frame = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
        let mut depth = vec![0.0f32; (WIDTH * HEIGHT) as usize];
        let mut ctx = DrawContext::new(&mut frame, &mut depth, WIDTH, HEIGHT, [0, 0, 0, 255]);

        draw_triangle_filled(
            vertex(-0.8, 0.0, 1.0, 1.0),
            vertex(0.8, 0.0, w_far, 0.0),
            vertex(0.8, 0.8, w_far, 0.0),
            &mut ctx,
        );

        frame
    }

    fn red_at(frame: &[u8], x: u32, y: u32) -> u8 {
        frame[((y * WIDTH + x) * 4) as usize]
    }

    /// Вес левой вершины в пикселе с данным x
    fn weight_of_near_vertex(x: u32) -> f32 {
        (90.0 - (x as f32 + 0.5)) / 80.0
    }

    #[test]
    fn equal_w_makes_interpolation_plain_linear() {
        // Все вершины на одной глубине — делить на 1/w нечего, поправка
        // обязана выродиться в обычное линейное смешивание
        let frame = render_gradient(1.0);

        for x in [50, 60, 70] {
            let expected = (weight_of_near_vertex(x) * 255.0) as u8;

            assert_eq!(red_at(&frame, x, 30), expected, "пиксель x={}", x);
        }
    }

    #[test]
    fn interpolation_is_perspective_correct() {
        // Дальние вершины отодвинуты в девять раз. Ожидание считаем по формуле
        // attr = sum(b*a/w) / sum(b/w) — независимо от кода растеризатора
        let frame = render_gradient(9.0);

        for x in [50, 60, 70] {
            let b0 = weight_of_near_vertex(x);
            let expected = (b0 / (b0 + (1.0 - b0) / 9.0) * 255.0) as u8;

            assert_eq!(red_at(&frame, x, 30), expected, "пиксель x={}", x);
        }
    }

    #[test]
    fn perspective_correction_pulls_color_towards_the_near_vertex() {
        // Смысл поправки: дальняя вершина занимает на экране меньше «своей»
        // поверхности, поэтому её вклад должен быть слабее линейного
        let linear = render_gradient(1.0);
        let corrected = render_gradient(9.0);

        for x in [50, 60, 70] {
            assert!(
                red_at(&corrected, x, 30) > red_at(&linear, x, 30) + 50,
                "в пикселе x={} поправка почти ничего не изменила: {} против {}",
                x,
                red_at(&corrected, x, 30),
                red_at(&linear, x, 30)
            );
        }
    }

    /// Тот же треугольник, что в `render_gradient`, но вершины несут UV, а не
    /// цвет: ближняя u = 0, дальние u = 1. Цвет везде белый, чтобы в кадре
    /// оказалась чистая текстура без примеси освещения.
    ///
    /// Текстура — два текселя: левая половина чёрная, правая красная. Значит
    /// на экране будет ровно одна граница, и её положение — это и есть
    /// измеренное значение u
    fn render_textured(w_far: f32) -> Vec<u8> {
        let texture = Texture::new(
            2,
            1,
            vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0)],
        );

        let white = Vec3::new(1.0, 1.0, 1.0);
        let textured = |ndc_x: f32, ndc_y: f32, w: f32, u: f32| {
            ShadedVertex::new(
                Vec4 {
                    x: ndc_x * w,
                    y: ndc_y * w,
                    z: 0.0,
                    w,
                },
                white,
            )
            .with_uv(Vec2::new(u, 0.0))
        };

        let mut frame = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
        let mut depth = vec![0.0f32; (WIDTH * HEIGHT) as usize];
        let mut ctx = DrawContext::new(&mut frame, &mut depth, WIDTH, HEIGHT, [0, 0, 0, 255]);
        ctx.texture = Some(&texture);

        draw_triangle_filled(
            textured(-0.8, 0.0, 1.0, 0.0),
            textured(0.8, 0.0, w_far, 1.0),
            textured(0.8, 0.8, w_far, 1.0),
            &mut ctx,
        );

        frame
    }

    /// Первый столбец строки, где текстура сменилась с чёрной на красную.
    /// Строка 45 выбрана так, чтобы весь диапазон 22..89 лежал внутри
    /// треугольника — иначе «не закрашено» спуталось бы с «чёрный тексель»
    fn texture_boundary_column(frame: &[u8]) -> u32 {
        (22..89)
            .find(|x| red_at(frame, *x, 45) > 0)
            .expect("красная половина текстуры не попала в кадр")
    }

    #[test]
    fn texture_lookup_is_perspective_correct() {
        // Ожидание считается из формулы, а не подсматривается у растеризатора.
        //
        // Дальние вершины отодвинуты в 9 раз. Перспективно верное значение
        // атрибута в пикселе: u = sum(b*u/w) / sum(b/w). Ближняя вершина имеет
        // u = 0 при w = 1, дальние — u = 1 при w = 9, значит
        //
        //     u = ((1 - b0) / 9) / (b0 + (1 - b0) / 9)
        //
        // где b0 — вес ближней вершины. Граница текселей на u = 0.5:
        //
        //     (1 - b0) / 9 = 0.5 * (b0 + (1 - b0) / 9)   ->   b0 = 0.1
        //
        // Обе дальние вершины стоят на общей вертикали x = 90, поэтому
        // b0 = (90 - x) / 80, и b0 = 0.1 попадает на x = 82. Первый красный
        // столбец — 82: у пикселя 81 центр 81.5, там b0 = 0.10625 и u < 0.5.
        //
        // Без поправки u был бы просто (1 - b0), граница легла бы на b0 = 0.5,
        // то есть на x = 50. Тридцать два столбца разницы — промахнуться
        // мимо такой поломки невозможно
        assert_eq!(texture_boundary_column(&render_textured(9.0)), 82);
    }

    #[test]
    fn equal_w_degenerates_the_texture_lookup_to_linear() {
        // Контроль к предыдущему тесту: если все вершины на одной глубине,
        // делить не на что и поправка обязана исчезнуть. Тогда u = 1 - b0,
        // граница на b0 = 0.5, то есть на x = 50: у пикселя 49 центр 49.5,
        // b0 = 0.50625 и u ещё меньше половины
        assert_eq!(texture_boundary_column(&render_textured(1.0)), 50);
    }

    #[test]
    fn texture_modulates_the_lit_color_instead_of_replacing_it() {
        // Полностью белая текстура на затенённой вдвое поверхности обязана
        // дать серый, а не белый. Иначе текстурированные объекты перестали бы
        // реагировать на свет, и вся сцена стала бы плоской
        let texture = Texture::new(1, 1, vec![Vec3::new(1.0, 1.0, 1.0)]);

        let half_lit = |ndc_x: f32, ndc_y: f32| {
            ShadedVertex::new(
                Vec4 {
                    x: ndc_x,
                    y: ndc_y,
                    z: 0.0,
                    w: 1.0,
                },
                Vec3::new(0.5, 0.5, 0.5),
            )
        };

        let mut frame = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
        let mut depth = vec![0.0f32; (WIDTH * HEIGHT) as usize];
        let mut ctx = DrawContext::new(&mut frame, &mut depth, WIDTH, HEIGHT, [0, 0, 0, 255]);
        ctx.texture = Some(&texture);

        draw_triangle_filled(
            half_lit(-0.8, -0.8),
            half_lit(0.8, -0.8),
            half_lit(0.8, 0.8),
            &mut ctx,
        );

        // 0.5 * 1.0 = 0.5 -> 127 (усечение, а не округление)
        assert_eq!(red_at(&frame, 70, 60), 127);
    }

    #[test]
    fn interpolated_color_never_overshoots_vertex_values() {
        // Деление на интерполированное 1/w не должно выбрасывать результат
        // за пределы отрезка между значениями в вершинах
        let frame = render_gradient(9.0);

        for pixel in frame.chunks_exact(4).filter(|p| p[3] != 0) {
            assert!(pixel[1] == 0 && pixel[2] == 0, "появился цвет из ниоткуда");
        }
    }
}
