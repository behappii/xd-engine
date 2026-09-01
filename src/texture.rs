//! Текстура и её выборка.
//!
//! Текстура — это просто картинка, которую растеризатор читает в каждом
//! пикселе по интерполированным UV-координатам. Вся хитрость не здесь, а в
//! том, КАК до неё доезжает UV: барицентрики считаются на экране, уже после
//! перспективного деления, поэтому интерполировать u и v напрямую нельзя —
//! см. `draw_triangle_filled`.

use crate::math::{Vec2, Vec3};

/// Растровая картинка, из которой берётся цвет поверхности.
///
/// Тексели хранятся уже в виде `Vec3` с компонентами 0..1, а не байтами.
/// Памяти это стоит втрое, но выборка происходит в самом горячем месте
/// движка — на каждый закрашенный пиксель, — и делить там три байта на 255
/// незачем: преобразование делается один раз при загрузке
#[derive(Debug, Clone)]
pub struct Texture {
    width: u32,
    height: u32,
    texels: Vec<Vec3>,
}

impl Texture {
    /// Собрать текстуру из готовых текселей построчно, слева направо
    /// и сверху вниз.
    ///
    /// Паникует на пустом размере и на несовпадении длины: текстура нулевой
    /// ширины сломала бы выборку делением на ноль, а «почти правильная»
    /// длина — это опечатка, которую лучше поймать сразу, чем разглядывать
    /// потом косые полосы на модели
    pub fn new(width: u32, height: u32, texels: Vec<Vec3>) -> Self {
        assert!(width > 0 && height > 0, "текстура нулевого размера");
        assert_eq!(
            texels.len(),
            (width * height) as usize,
            "текселей не столько, сколько обещает размер"
        );

        Self {
            width,
            height,
            texels,
        }
    }

    /// Собрать текстуру из потока байтов RGBA — того вида, в котором картинка
    /// приезжает из файла. Альфа отбрасывается: смешивания в пайплайне нет
    pub fn from_rgba8(width: u32, height: u32, bytes: &[u8]) -> Self {
        let texels = bytes
            .chunks_exact(4)
            .map(|p| {
                Vec3::new(
                    p[0] as f32 / 255.0,
                    p[1] as f32 / 255.0,
                    p[2] as f32 / 255.0,
                )
            })
            .collect();

        Self::new(width, height, texels)
    }

    /// Загрузить текстуру из файла картинки (PNG или JPEG).
    ///
    /// Единственное место в проекте, где сознательно взят внешний крейт:
    /// внутри PNG лежит поток, сжатый zlib (Хаффман + LZ77), и свой
    /// распаковщик — это неделя работы над архиватором, а не над движком.
    ///
    /// Путь считается от каталога, откуда запущен процесс; `cargo run` берёт
    /// корень проекта, поэтому `"textures/floor.png"` работает как есть
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self, image::ImageError> {
        Ok(Self::from_dynamic(image::open(path)?))
    }

    /// То же, но из байтов, уже лежащих в памяти: формат определяется по
    /// сигнатуре в начале данных, а не по расширению.
    ///
    /// Нужно для `include_bytes!` — тогда картинка вшивается в исполняемый
    /// файл и не зависит от рабочего каталога
    pub fn decode(bytes: &[u8]) -> Result<Self, image::ImageError> {
        Ok(Self::from_dynamic(image::load_from_memory(bytes)?))
    }

    /// Общий хвост обоих загрузчиков.
    ///
    /// `into_rgba8` приводит что угодно — палитру, оттенки серого, 16 бит на
    /// канал — к единственному раскладу, который понимает `from_rgba8`.
    /// Строки в нём идут сверху вниз, то есть ровно так, как ждёт `sample`:
    /// переворачивать ничего не надо.
    ///
    /// Чего этот путь НЕ делает — не смотрит в EXIF. Фотография с телефона,
    /// снятая боком, лежит в файле как есть, а «правильный» поворот записан
    /// отдельным тегом; на текстуре это выйдет картинкой набок
    fn from_dynamic(image: image::DynamicImage) -> Self {
        let rgba = image.into_rgba8();

        Self::from_rgba8(rgba.width(), rgba.height(), rgba.as_raw())
    }

    /// Шахматка `cells`×`cells` клеток на квадрате `size`×`size` текселей.
    ///
    /// Процедурная текстура нужна раньше загрузки картинок из файла, и не для
    /// красоты: на шахматке перспективная коррекция видна безошибочно. Если
    /// UV интерполировать линейно по экрану, клетки на грани, повёрнутой к
    /// камере под углом, изогнутся — на градиенте такое можно и проглядеть,
    /// на прямых линиях нет
    pub fn checker(size: u32, cells: u32, a: [u8; 4], b: [u8; 4]) -> Self {
        let size = size.max(1);
        let cells = cells.max(1);

        let color_a = unpack(a);
        let color_b = unpack(b);

        let mut texels = Vec::with_capacity((size * size) as usize);

        for y in 0..size {
            for x in 0..size {
                // Номер клетки: сначала умножаем, потом делим. Наоборот
                // (x / (size / cells)) было бы неверно — при size, не кратном
                // cells, сторона клетки в текселях дробная, и целочисленное
                // size / cells её теряет; 10 текселей на 4 клетки дали бы
                // размер 2 вместо 2.5 и лишнюю пятую клетку с краю
                let cell_x = x * cells / size;
                let cell_y = y * cells / size;

                // Соседние по диагонали клетки одного цвета — отсюда чётность суммы
                texels.push(if (cell_x + cell_y).is_multiple_of(2) {
                    color_a
                } else {
                    color_b
                });
            }
        }

        Self::new(size, size, texels)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Цвет текселя по текстурной координате.
    ///
    /// Соглашение: u растёт вправо, v — ВНИЗ, то есть v = 0 это верхняя строка
    /// картинки. Так лежат строки в файле, и так же считает большинство
    /// форматов; вертикальные перевороты — самый частый источник «текстура
    /// вверх ногами».
    ///
    /// Выборка ближайшего соседа: берём тексель, в который попала координата,
    /// без сглаживания между соседями. Отсюда крупные квадраты вблизи и рябь
    /// вдали — билинейная фильтрация и мип-уровни это лечат, но их пока нет.
    ///
    /// Координаты за пределами 0..1 заворачиваются (режим repeat): u = 1.25
    /// это то же, что 0.25. Благодаря этому UV в развёртке можно умножать на
    /// число и получать плитку
    #[inline]
    pub fn sample(&self, uv: Vec2) -> Vec3 {
        // rem_euclid, а не `%`: остаток в Rust сохраняет знак делимого, и
        // (-0.25) % 1.0 дало бы -0.25, то есть отрицательный индекс. Нужен
        // именно неотрицательный остаток
        let u = uv.x.rem_euclid(1.0);
        let v = uv.y.rem_euclid(1.0);

        // NaN сюда доехать не должен, но если доедет — rem_euclid вернёт NaN,
        // `as u32` даст 0, и вместо паники будет левый верхний тексель
        let x = (u * self.width as f32) as u32;
        let y = (v * self.height as f32) as u32;

        // Зажим обязателен, и не для красоты: rem_euclid на крошечном
        // отрицательном входе возвращает ровно 1.0 (результат округляется
        // вверх до делителя), а 1.0 * width — это индекс за границей строки
        let x = x.min(self.width - 1);
        let y = y.min(self.height - 1);

        self.texels[(y * self.width + x) as usize]
    }
}

/// Цвет из байтов в вектор с компонентами 0..1
fn unpack(color: [u8; 4]) -> Vec3 {
    Vec3::new(
        color[0] as f32 / 255.0,
        color[1] as f32 / 255.0,
        color[2] as f32 / 255.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-5;

    /// Текстура 2×1: левый тексель красный, правый зелёный
    fn two_texels() -> Texture {
        Texture::new(
            2,
            1,
            vec![Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)],
        )
    }

    fn assert_same(actual: Vec3, expected: Vec3) {
        assert!(
            (actual - expected).length() < EPS,
            "получили {:?}, ждали {:?}",
            actual,
            expected
        );
    }

    #[test]
    fn sample_picks_the_texel_the_coordinate_falls_into() {
        let tex = two_texels();

        // Граница между текселями ровно на u = 0.5, левый тексель — [0, 0.5)
        assert_same(tex.sample(Vec2::new(0.0, 0.5)), Vec3::new(1.0, 0.0, 0.0));
        assert_same(tex.sample(Vec2::new(0.49, 0.5)), Vec3::new(1.0, 0.0, 0.0));
        assert_same(tex.sample(Vec2::new(0.5, 0.5)), Vec3::new(0.0, 1.0, 0.0));
        assert_same(tex.sample(Vec2::new(0.99, 0.5)), Vec3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn coordinates_outside_the_unit_square_wrap_around() {
        let tex = two_texels();

        // u = 1.0 — это уже следующая копия текстуры, то есть снова её начало
        assert_same(
            tex.sample(Vec2::new(1.0, 0.0)),
            tex.sample(Vec2::new(0.0, 0.0)),
        );
        assert_same(
            tex.sample(Vec2::new(3.75, 0.0)),
            tex.sample(Vec2::new(0.75, 0.0)),
        );

        // Отрицательные — та же плитка в другую сторону. Здесь `%` вместо
        // rem_euclid дал бы отрицательный индекс и панику
        assert_same(
            tex.sample(Vec2::new(-0.25, 0.0)),
            tex.sample(Vec2::new(0.75, 0.0)),
        );
        assert_same(
            tex.sample(Vec2::new(-1.25, 0.0)),
            tex.sample(Vec2::new(0.75, 0.0)),
        );
    }

    #[test]
    fn wrapping_never_reads_outside_the_buffer() {
        // Тот самый случай, ради которого стоит зажим: rem_euclid от
        // крошечного отрицательного числа округляется вверх ровно до 1.0
        let tex = two_texels();

        assert_eq!(
            (-1e-9f32).rem_euclid(1.0),
            1.0,
            "предпосылка теста изменилась"
        );

        // Паники быть не должно, и это должен быть последний тексель строки
        assert_same(
            tex.sample(Vec2::new(-1e-9, -1e-9)),
            Vec3::new(0.0, 1.0, 0.0),
        );
    }

    #[test]
    fn v_grows_downwards() {
        // Столбец из двух текселей: верхний белый, нижний чёрный.
        // v = 0 обязано попасть в верхний, иначе текстуры окажутся
        // перевёрнутыми, а это самая обидная и самая незаметная ошибка
        let tex = Texture::new(
            1,
            2,
            vec![Vec3::new(1.0, 1.0, 1.0), Vec3::new(0.0, 0.0, 0.0)],
        );

        assert_same(tex.sample(Vec2::new(0.0, 0.0)), Vec3::new(1.0, 1.0, 1.0));
        assert_same(tex.sample(Vec2::new(0.0, 0.9)), Vec3::new(0.0, 0.0, 0.0));
    }

    #[test]
    fn checker_alternates_along_both_axes() {
        // 4×4 текселя, 2×2 клетки — каждая клетка ровно 2×2 текселя
        let tex = Texture::checker(4, 2, [255, 255, 255, 255], [0, 0, 0, 255]);

        let at = |u: f32, v: f32| tex.sample(Vec2::new(u, v)).x;

        // Углы по диагонали одного цвета, по стороне — разного
        assert_eq!(at(0.1, 0.1), 1.0);
        assert_eq!(at(0.9, 0.1), 0.0);
        assert_eq!(at(0.1, 0.9), 0.0);
        assert_eq!(at(0.9, 0.9), 1.0);
    }

    #[test]
    fn checker_cells_are_all_the_same_size_even_when_size_is_not_divisible() {
        // 10 текселей на 4 клетки: 2.5 текселя на клетку. Наивное size/cells
        // дало бы целочисленный ноль и одну сплошную заливку
        let tex = Texture::checker(10, 4, [255, 255, 255, 255], [0, 0, 0, 255]);

        let row: Vec<f32> = (0..10)
            .map(|x| tex.sample(Vec2::new((x as f32 + 0.5) / 10.0, 0.0)).x)
            .collect();

        // Границы клеток на 2.5, 5.0, 7.5 текселя
        assert_eq!(row, vec![1.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0]);
    }

    /// PNG 2×2, собранный в памяти: верхняя строка красный/зелёный,
    /// нижняя — синий/белый
    fn sample_png() -> Vec<u8> {
        #[rustfmt::skip]
        let pixels: Vec<u8> = vec![
            255, 0, 0, 255,   0, 255, 0, 255,
            0, 0, 255, 255,   255, 255, 255, 255,
        ];

        let source = image::RgbaImage::from_raw(2, 2, pixels).unwrap();

        let mut png = Vec::new();
        source
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();

        png
    }

    #[test]
    fn decoding_a_png_keeps_rows_top_down_and_channels_in_order() {
        // Ради этого теста стоило собирать картинку в памяти: он ловит сразу
        // два самых частых промаха загрузчика — перевёрнутые строки (v = 0
        // обязано попасть в верхнюю) и перепутанные R и B
        let tex = Texture::decode(&sample_png()).unwrap();

        assert_eq!((tex.width(), tex.height()), (2, 2));

        assert_same(tex.sample(Vec2::new(0.25, 0.25)), Vec3::new(1.0, 0.0, 0.0));
        assert_same(tex.sample(Vec2::new(0.75, 0.25)), Vec3::new(0.0, 1.0, 0.0));
        assert_same(tex.sample(Vec2::new(0.25, 0.75)), Vec3::new(0.0, 0.0, 1.0));
        assert_same(tex.sample(Vec2::new(0.75, 0.75)), Vec3::new(1.0, 1.0, 1.0));
    }

    #[test]
    fn a_broken_file_is_an_error_rather_than_a_panic() {
        // Картинка приходит извне, и битый или просто не тот файл — обычное
        // дело. Загрузчик обязан вернуть Err, а не уронить процесс
        assert!(Texture::decode("это не картинка".as_bytes()).is_err());
        assert!(Texture::load("textures/такого-файла-нет.png").is_err());
    }

    #[test]
    fn from_rgba8_reads_rows_left_to_right_top_to_bottom() {
        let tex = Texture::from_rgba8(2, 1, &[255, 0, 0, 255, /**/ 0, 0, 255, 255]);

        assert_same(tex.sample(Vec2::new(0.25, 0.0)), Vec3::new(1.0, 0.0, 0.0));
        assert_same(tex.sample(Vec2::new(0.75, 0.0)), Vec3::new(0.0, 0.0, 1.0));
    }
}
