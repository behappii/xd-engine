use crate::{
    clipping::clip_line_4d,
    config::EPSILON,
    math::{Vec3, Vec4},
};

pub struct DrawContext<'frame> {
    pub frame: &'frame mut [u8],
    pub depth: &'frame mut [f32],
    pub width: u32,
    pub height: u32,
    pub color: [u8; 4],
}

impl<'frame> DrawContext<'frame> {
    pub fn new(
        frame: &'frame mut [u8],
        depth: &'frame mut [f32],
        width: u32,
        height: u32,
        color: [u8; 4],
    ) -> Self {
        Self {
            frame,
            depth,
            width,
            height,
            color,
        }
    }

    /// Единственное место, где считается индекс пикселя и проверяются границы
    /// Запись без теста глубины (для проволочных линий)
    #[inline]
    pub fn set_pixel(&mut self, x: i32, y: i32) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }

        let index = (y as usize * self.width as usize + x as usize) * 4;
        self.frame[index..index + 4].copy_from_slice(&self.color);
    }

    /// Запись с тестом глубины. `inv_w` — величина 1/w: чем больше, тем ближе.
    /// Координаты уже проверены растеризатором, границы не трогаем.
    #[inline]
    pub fn set_pixel_depth(&mut self, x: usize, y: usize, inv_w: f32) {
        let i = y * self.width as usize + x;

        if inv_w <= self.depth[i] {
            return;
        }

        self.depth[i] = inv_w;
        let p = i * 4;
        self.frame[p..p + 4].copy_from_slice(&self.color);
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

/// NDC -> экран, но в f32: округление здесь убило бы точность краёв
fn ndc_to_screen_f(x: f32, y: f32, width: u32, height: u32) -> (f32, f32) {
    (
        (x + 1.0) * 0.5 * width as f32,
        (1.0 - y) * 0.5 * height as f32,
    )
}

/// Знаковая площадь параллелограмма на векторах (a->b) и (a->p)
#[inline]
fn edge(ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32) -> f32 {
    (bx - ax) * (py - ay) - (by - ay) * (px - ax)
}

pub fn draw_triangle_wireframe(v0: Vec4, v1: Vec4, v2: Vec4, ctx: &mut DrawContext) {
    draw_clipped_edge(v0, v1, ctx);
    draw_clipped_edge(v1, v2, ctx);
    draw_clipped_edge(v2, v0, ctx);
}

pub fn draw_triangle_filled(v0: Vec4, v1: Vec4, v2: Vec4, ctx: &mut DrawContext) {
    // Клиппинг гарантировал w > 0
    let iw0 = 1.0 / v0.w;
    let iw1 = 1.0 / v1.w;
    let iw2 = 1.0 / v2.w;

    let (x0, y0) = ndc_to_screen_f(v0.x * iw0, v0.y * iw0, ctx.width, ctx.height);
    let (x1, y1) = ndc_to_screen_f(v1.x * iw1, v1.y * iw1, ctx.width, ctx.height);
    let (x2, y2) = ndc_to_screen_f(v2.x * iw2, v2.y * iw2, ctx.width, ctx.height);

    let area = edge(x0, y0, x1, y1, x2, y2);

    // Вырожденный треугольник (грань ровно ребром к камере)
    if area.abs() < EPSILON {
        return;
    }

    // Знак зависит от обхода; ndc_to_screen переворачивает Y, поэтому
    // не гадаем, а нормируем по фактическому знаку
    let sign = if area > 0.0 { 1.0 } else { -1.0 };
    let inv_area = 1.0 / area.abs();

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

            ctx.set_pixel_depth(x, y, inv_w);
        }
    }
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
