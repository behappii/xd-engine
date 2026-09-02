use crate::{clipping::clip_line_4d, math::Vec4};

use super::{
    DrawContext,
    screen::{clip_to_ndc, ndc_to_screen},
};

pub fn draw_triangle_wireframe(v0: Vec4, v1: Vec4, v2: Vec4, ctx: &mut DrawContext) {
    draw_clipped_edge(v0, v1, ctx);
    draw_clipped_edge(v1, v2, ctx);
    draw_clipped_edge(v2, v0, ctx);
}

fn draw_clipped_edge(start: Vec4, end: Vec4, ctx: &mut DrawContext) {
    if let Some((c0, c1)) = clip_line_4d(start, end) {
        let ndc0 = clip_to_ndc(c0);
        let ndc1 = clip_to_ndc(c1);

        let p0 = ndc_to_screen(ndc0, ctx.width, ctx.height);

        let p1 = ndc_to_screen(ndc1, ctx.width, ctx.height);

        // Клиппер уже отрезал всё, что за ближней плоскостью, поэтому w > 0
        // и делить можно без проверки. 1/w — та же величина, что лежит в
        // depth-буфере: чем больше, тем ближе
        draw_line(p0, 1.0 / c0.w, p1, 1.0 / c1.w, ctx);
    }
}

fn draw_line(from: (i32, i32), inv_w0: f32, to: (i32, i32), inv_w1: f32, ctx: &mut DrawContext) {
    let (x0, y0) = from;
    let (x1, y1) = to;

    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx - dy;

    // Сколько будет шагов, известно заранее: Брезенхем идёт по длинной оси
    // ровно по пикселю за шаг, поэтому их max(dx, dy), а точек на единицу
    // больше. По этому счётчику и тянется глубина.
    //
    // Интерполируется именно 1/w, а не w: на ЭКРАНЕ линейна только она —
    // ровно та же причина, по которой в залитом треугольнике интерполируются
    // attr/w и 1/w, а не сами атрибуты. Доля пути вдоль длинной оси при этом
    // корректный параметр: прямая на экране остаётся прямой, и шаг по этой
    // оси линейно связан с положением на ней
    let steps = dx.max(dy);
    // Линия могла схлопнуться в точку — тогда делить не на что, да и незачем
    let step = if steps > 0 {
        (inv_w1 - inv_w0) / steps as f32
    } else {
        0.0
    };

    let mut x = x0;
    let mut y = y0;
    let mut done = 0;

    loop {
        // Отсчёт от начала, а не накопление `inv_w += step`: у накопления
        // ошибка f32 растёт вдоль линии, а здесь умножение всего одно
        ctx.set_pixel(x, y, inv_w0 + step * done as f32);

        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 > -dy {
            err -= dy;
            x += sx;
        }
        if e2 < dx {
            err += dx;
            y += sy;
        }

        done += 1;
    }
}
