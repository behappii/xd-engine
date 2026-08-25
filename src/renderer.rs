pub const WIDTH: u32 = 800;
pub const HEIGHT: u32 = 600;

pub fn draw_line(x0: i32, y0: i32, x1: i32, y1: i32, frame: &mut [u8]) {
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
            frame[pixel_index] = 0; // R
            frame[pixel_index + 1] = 255; // G
            frame[pixel_index + 2] = 0; // B
            frame[pixel_index + 3] = 255; // A
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
