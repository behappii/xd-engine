use crate::{
    clipping::clip_line_4d,
    config::{HEIGHT, LINE_COLOR, WIDTH},
    math::{Vec3, Vec4},
};

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

pub fn draw_triangle_wireframe(v0: Vec4, v1: Vec4, v2: Vec4, frame: &mut [u8]) {
    draw_clipped_egde(v0, v1, frame);
    draw_clipped_egde(v1, v2, frame);
    draw_clipped_egde(v2, v0, frame);
}

fn draw_clipped_egde(start: Vec4, end: Vec4, frame: &mut [u8]) {
    if let Some((c0, c1)) = clip_line_4d(start, end) {
        let ndc0 = clip_to_ndc(c0);
        let ndc1 = clip_to_ndc(c1);

        let p0 = ndc_to_screen(ndc0, WIDTH, HEIGHT);

        let p1 = ndc_to_screen(ndc1, WIDTH, HEIGHT);

        draw_line(p0.0, p0.1, p1.0, p1.1, frame);
    }
}

fn draw_line(x0: i32, y0: i32, x1: i32, y1: i32, frame: &mut [u8]) {
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx - dy;

    let mut x = x0;
    let mut y = y0;

    let mut iterations = 0;

    loop {
        iterations += 1;

        if iterations > 10000 {
            println!("line stuck: ({},{}) -> ({},{})", x0, y0, x1, y1);
            break;
        }

        if x >= 0 && x < WIDTH as i32 && y >= 0 && y < HEIGHT as i32 {
            let pixel_index = ((y as usize) * (WIDTH as usize) + (x as usize)) * 4;
            frame[pixel_index] = LINE_COLOR[0]; // R
            frame[pixel_index + 1] = LINE_COLOR[1]; // G
            frame[pixel_index + 2] = LINE_COLOR[2]; // B
            frame[pixel_index + 3] = LINE_COLOR[3]; // A
        }

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
