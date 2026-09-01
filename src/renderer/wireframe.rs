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

        draw_line(p0.0, p0.1, p1.0, p1.1, ctx);
    }
}

fn draw_line(x0: i32, y0: i32, x1: i32, y1: i32, ctx: &mut DrawContext) {
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx - dy;

    let mut x = x0;
    let mut y = y0;

    loop {
        ctx.set_pixel(x, y);

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
    }
}
