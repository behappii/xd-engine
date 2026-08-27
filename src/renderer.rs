use crate::{
    clipping::clip_line_4d,
    math::{Vec3, Vec4},
};

pub struct DrawContext<'frame> {
    pub frame: &'frame mut [u8],
    pub width: u32,
    pub height: u32,
    pub color: [u8; 4],
}

impl<'frame> DrawContext<'frame> {
    pub fn new(frame: &'frame mut [u8], width: u32, height: u32, color: [u8; 4]) -> Self {
        Self {
            frame,
            width,
            height,
            color,
        }
    }

    /// Единственное место, где считается индекс пикселя и проверяются границы
    #[inline]
    pub fn set_pixel(&mut self, x: i32, y: i32) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }

        let index = (y as usize * self.width as usize + x as usize) * 4;
        self.frame[index..index + 4].copy_from_slice(&self.color);
    }
}

// Perspective Divide
fn clip_to_ndc(v: Vec4) -> Vec3 {
    Vec3 {
        x: v.x / v.w,
        y: v.y / v.w,
        z: v.z / v.w,
    }
}

// Viewport Transform
fn ndc_to_screen(v: Vec3, width: u32, height: u32) -> (i32, i32) {
    let x = ((v.x + 1.0) * 0.5 * width as f32).round() as i32;

    let y = ((1.0 - v.y) * 0.5 * height as f32).round() as i32;

    (x, y)
}

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

/// Заливка всего буфера одним цветом
pub fn clear_frame(frame: &mut [u8], color: [u8; 4]) {
    if frame.is_empty() {
        return;
    }

    // Быстрый путь: если все четыре байта одинаковые, это обычный memset
    if color[0] == color[1] && color[1] == color[2] && color[2] == color[3] {
        frame.fill(color[0]);
        return;
    }

    // Общий случай: пишем один пиксель, дальше удваиваем уже заполненный
    // участок через memcpy — за log2(N) итераций
    frame[0..4].copy_from_slice(&color);

    let mut filled = 4;
    while filled < frame.len() {
        let chunk = filled.min(frame.len() - filled);
        frame.copy_within(0..chunk, filled);
        filled += chunk;
    }
}
