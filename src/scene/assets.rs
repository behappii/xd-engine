use std::path::{Path, PathBuf};

use crate::texture::{Magnify, Minify, Texture};

use super::Mesh;

/// Ссылка на меш, живущий в сцене.
///
/// Внутри обычный индекс, поэтому тип `Copy`: объявил меш один раз, дальше
/// раздавай сколько угодно инстансам без всяких `clone`. Раньше на этом месте
/// был `Rc<Mesh>`, и у него было ровно два минуса. Пользовательский: чтобы
/// переиспользовать меш, приходилось писать `Rc::new` и `Rc::clone` руками.
/// И технический, куда более неприятный: `Rc` не `Sync`, потому что счётчик
/// ссылок у него неатомарный, а значит `&Instance` нельзя было отдать в
/// другой поток — вершинный этап оставался однопоточным.
///
/// Плата за простоту: типом ничего не гарантируется. Индекс из чужой сцены
/// скомпилируется и молча возьмёт не тот меш
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshId(usize);

/// Ссылка на текстуру, живущую в сцене. Всё то же, что и у [`MeshId`]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureId(usize);

/// Арены мешей и текстур: всё, что дорого создавать и не принадлежит одной
/// конкретной сцене.
///
/// Отделено от [`super::Scene`] потому, что у ресурсов и у мира разные сроки жизни.
/// Главное меню и уровень — это две сцены, но куб в них один и тот же, и
/// пересоздавать его при переходе бессмысленно. Пока арены жили внутри сцены,
/// смена сцены выбрасывала вместе с миром все меши и текстуры, а внутриигровое
/// меню поверх игры было не сделать вовсе: чтобы показать меню, пришлось бы
/// уничтожить мир вместе с состоянием игры.
///
/// Второе следствие важнее на вид: `MeshId` теперь действителен везде. Пока
/// арена была у каждой сцены своя, индекс из одной сцены, применённый в
/// другой, молча брал не тот меш — с общими аренами «чужой сцены» просто нет.
///
/// Разделяемых указателей внутри нет — только `Vec` простых данных, — поэтому
/// `&Assets` можно одолжить сразу нескольким потокам
#[derive(Debug, Clone)]
pub struct Assets {
    meshes: Vec<Mesh>,
    textures: Vec<Texture>,

    /// От чего считаются относительные пути к файлам. Как он выбирается —
    /// в доке `default_asset_root` ниже
    asset_root: PathBuf,
}

impl Default for Assets {
    fn default() -> Self {
        Self {
            meshes: Vec::new(),
            textures: Vec::new(),
            asset_root: default_asset_root(),
        }
    }
}

impl Assets {
    pub fn new() -> Self {
        Self::default()
    }

    /// Каталог, от которого считаются относительные пути к ассетам
    pub fn asset_root(&self) -> &Path {
        &self.asset_root
    }

    /// Задать корень ассетов вручную.
    ///
    /// Автоматика покрывает два обычных случая —
    /// запуск через cargo и собранную игру рядом со своими файлами. Ручная
    /// установка нужна, если файлы лежат иначе: в каталоге пользователя,
    /// в распакованном архиве мода, в каталоге из аргументов командной строки
    pub fn set_asset_root(&mut self, root: impl Into<PathBuf>) {
        self.asset_root = root.into();
    }

    /// Загрузить картинку из файла, настроить фильтрацию и зарегистрировать —
    /// одним вызовом.
    ///
    /// # Параметры
    ///
    /// - `path` — путь к файлу. ОТНОСИТЕЛЬНЫЙ считается от корня ассетов, а не
    ///   от рабочего каталога процесса. Разница не косметическая: рабочий
    ///   каталог — это откуда игру запустили, и у собранной игры он какой
    ///   угодно. Абсолютный путь берётся как есть.
    /// - `magnify`, `minify` — фильтрация, как у [`Texture::with_filter`].
    ///   Параметрами, а не умолчанием: у загруженной из файла картинки
    ///   умолчание `Nearest`/`Nearest` почти наверняка не то, что нужно, —
    ///   фотографию хочется сгладить.
    ///
    /// # Если файла нет
    ///
    /// Ничего не происходит: в лог уходит предупреждение, а на объект
    /// становится [`Texture::missing`] — малиново-чёрная клетка. `Result`
    /// тут намеренно нет, и это не небрежность, а то, как устроены настоящие
    /// движки.
    ///
    /// Причина простая. Текстур в игре сотни, и обвязка «а вдруг не
    /// загрузилась» на каждой превращает код сцены в кашу — притом что
    /// осмысленно обработать эту ошибку игре всё равно нечем: рисовать-то
    /// надо. Заглушка решает обе задачи разом. Она громкая: малиновую клетку
    /// на модели видно мгновенно, и сразу ясно, что дело не в развёртке и не
    /// в свете, а в пропавшем файле. И она не тихая: предупреждение с путём и
    /// причиной идёт в stderr.
    ///
    /// Кому нужна именно ошибка — есть [`Assets::try_load_texture`].
    ///
    /// # Пример
    ///
    /// ```no_run
    /// use xd_engine::{scene::Assets, texture::{Magnify, Minify}};
    ///
    /// let mut assets = Assets::new();
    /// let wall = assets.load_texture("textures/wall.png", Magnify::Linear, Minify::Mipmapped);
    /// ```
    pub fn load_texture(
        &mut self,
        path: impl AsRef<Path>,
        magnify: Magnify,
        minify: Minify,
    ) -> TextureId {
        let path = path.as_ref();

        match self.try_load_texture(path, magnify, minify) {
            Ok(id) => id,
            Err(err) => {
                eprintln!(
                    "xd_engine: текстура «{}» не загрузилась ({err}) — подставлена заглушка",
                    path.display()
                );

                // Заглушку нарочно не фильтруем: она должна выглядеть
                // одинаково всегда и ни при каком масштабе не сойти за
                // настоящую картинку
                self.add_texture(Texture::missing())
            }
        }
    }

    /// То же, что и load_texture, но с ошибкой вместо заглушки.
    ///
    /// Нужен там, где отсутствующий файл — это не «художник не дорисовал», а
    /// настоящий сбой: проверка целостности установки, редактор уровней,
    /// загрузчик, который обязан отчитаться пользователю.
    ///
    /// # Ошибки
    ///
    /// Файла нет, нет прав, формат не тот, данные битые
    pub fn try_load_texture(
        &mut self,
        path: impl AsRef<Path>,
        magnify: Magnify,
        minify: Minify,
    ) -> Result<TextureId, image::ImageError> {
        // `join` сам разбирается с абсолютным путём: если он абсолютный,
        // корень отбрасывается целиком
        let full = self.asset_root.join(path);
        let texture = Texture::load(full)?.with_filter(magnify, minify);

        Ok(self.add_texture(texture))
    }

    /// Отдать меш аренам и получить ссылку на него.
    ///
    /// Ссылку можно копировать сколько угодно: она `Copy`, и каждый инстанс
    /// хранит у себя всего одно число
    pub fn add_mesh(&mut self, mesh: Mesh) -> MeshId {
        self.meshes.push(mesh);

        MeshId(self.meshes.len() - 1)
    }

    /// То же для текстуры.
    ///
    /// Заодно достраивает мип-пирамиду, если фильтр текстуры её просит. Место
    /// для этого правильное: регистрация — граница, на которой картинка входит
    /// в движок, аналог загрузки текстуры на видеокарту, где мипы и
    /// генерируются в настоящих движках.
    ///
    /// Раньше строить пирамиду приходилось руками, и это была ловушка:
    /// попросил `Minify::Mipmapped`, забыл `with_mipmaps` — и получил тихий
    /// алиасинг вместо ошибки. Забыть теперь негде.
    ///
    /// Обратно к «фильтр сам выделяет память» это НЕ откат. Разница в том, где
    /// стоит решение: `with_filter` по-прежнему ничего не строит и остаётся
    /// чистой настройкой, а память выделяется один раз, на входе в движок, где
    /// намерение уже известно целиком
    pub fn add_texture(&mut self, texture: Texture) -> TextureId {
        let texture = if texture.minify() == Minify::Mipmapped {
            texture.with_mipmaps()
        } else {
            // Пирамида нужна не всем: у текстуры без сжатия — отладочной
            // шахматки, атласа интерфейса — она треть памяти впустую
            texture
        };

        self.textures.push(texture);

        TextureId(self.textures.len() - 1)
    }

    pub fn mesh(&self, id: MeshId) -> &Mesh {
        &self.meshes[id.0]
    }

    /// Изменить меш на месте — например деформировать вершины.
    ///
    /// Правка достанется ВСЕМ инстансам с этим `MeshId`, во всех сценах.
    /// Чтобы изменить только один объект, зарегистрируй копию:
    /// `let id = assets.add_mesh(assets.mesh(base).clone())`
    pub fn mesh_mut(&mut self, id: MeshId) -> &mut Mesh {
        &mut self.meshes[id.0]
    }

    pub fn texture(&self, id: TextureId) -> &Texture {
        &self.textures[id.0]
    }
}

/// Откуда считать относительные пути к ассетам.
///
/// Проблема, которую это решает: `Texture::load` раньше отсчитывал путь от
/// РАБОЧЕГО КАТАЛОГА процесса. Под `cargo run` он случайно совпадает с корнем
/// проекта, и всё работает; у собранной игры это каталог, откуда её запустили,
/// то есть какой угодно, — и та же самая игра «в сборке просто не работает».
///
/// Правило из двух шагов.
///
/// **Разработка.** Cargo кладёт `CARGO_MANIFEST_DIR` в окружение ЗАПУСКАЕМОГО
/// процесса — и при `cargo run`, и при `cargo test`. Указывает она на корень
/// крейта ИГРЫ, потому что запускает cargo именно её. У собранного бинарника,
/// запущенного напрямую, переменной нет вовсе, так что она же и отличает
/// разработку от поставки. Замерено, не предположено.
///
/// Компилируемый `env!("CARGO_MANIFEST_DIR")` тут не годится категорически:
/// он вшивается в тот крейт, где написан, то есть в ДВИЖОК, и указывал бы на
/// его исходники где-нибудь в `~/.cargo/registry`.
///
/// **Поставка.** Каталог рядом с исполняемым файлом. Именно там лежат ассеты
/// у распакованной игры, и именно этим он отличается от рабочего каталога.
fn default_asset_root() -> PathBuf {
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        return PathBuf::from(manifest);
    }

    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        // Узнать собственный путь не вышло — остаётся рабочий каталог.
        // Хуже, чем ничего, не будет: это ровно прежнее поведение
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Vec2;
    use crate::texture::Magnify;

    /// Пустой каталог под файлы одного теста. Имя своё у каждого, чтобы
    /// тесты не мешали друг другу: `cargo test` гоняет их параллельно
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("xd_engine_{tag}"));

        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("textures")).unwrap();

        dir
    }

    /// Настоящий PNG 2x1: слева красный, справа синий
    fn write_png(path: &Path) {
        let pixels = vec![255, 0, 0, 255, /**/ 0, 0, 255, 255];
        let source = image::RgbaImage::from_raw(2, 1, pixels).unwrap();

        source.save(path).unwrap();
    }

    #[test]
    fn a_relative_path_is_resolved_against_the_asset_root() {
        // Ради этого всё и затевалось. Файл лежит во временном каталоге, а
        // рабочий каталог у теста — корень проекта, где такого файла нет.
        // Значит успех тут доказывает ровно одно: путь считался ОТ КОРНЯ
        // АССЕТОВ, а не от того места, откуда запустили процесс
        let root = scratch("relative");
        write_png(&root.join("textures/probe.png"));

        let mut assets = Assets::new();
        assets.set_asset_root(&root);

        let id = assets
            .try_load_texture("textures/probe.png", Magnify::Linear, Minify::Linear)
            .expect("файл на месте, путь обязан разрешиться");

        assert_eq!(
            (assets.texture(id).width(), assets.texture(id).height()),
            (2, 1)
        );

        // Тот же путь без подмены корня не находится — иначе тест доказывал бы
        // не то, что нужно, а просто что файл где-то есть
        assert!(
            Assets::new()
                .try_load_texture("textures/probe.png", Magnify::Linear, Minify::Linear)
                .is_err(),
            "файл нашёлся и без корня — тест ничего не проверяет"
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_absolute_path_ignores_the_root() {
        // Абсолютный путь пользователь посчитал сам, и подставлять к нему
        // корень было бы порчей. За это отвечает Path::join, но полагаться
        // на память о его поведении не стоит — проверим
        let root = scratch("absolute");
        let file = root.join("textures/probe.png");
        write_png(&file);

        let mut assets = Assets::new();
        assets.set_asset_root("/заведомо/несуществующий/корень");

        assert!(
            assets
                .try_load_texture(&file, Magnify::Nearest, Minify::Nearest)
                .is_ok(),
            "абсолютный путь склеили с корнем"
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_missing_file_gives_the_placeholder_instead_of_a_result() {
        // Главное решение всего этого метода: текстур в игре сотни, и обвязка
        // «а вдруг не загрузилась» на каждой превращает код сцены в кашу —
        // притом что осмысленно обработать ошибку игре нечем, рисовать-то
        // надо. Поэтому не Result, а громкая заглушка
        let mut assets = Assets::new();

        let id = assets.load_texture("такого-файла-нет.png", Magnify::Nearest, Minify::Nearest);

        let placeholder = Texture::missing();
        let got = assets.texture(id);

        assert_eq!(
            (got.width(), got.height()),
            (placeholder.width(), placeholder.height())
        );

        // Малиновая клетка, а не что-нибудь правдоподобное: увидев её на
        // модели, сразу понимаешь, что дело в пропавшем файле. Vec3 не
        // PartialEq — сравниваем покомпонентно
        let rgb = |t: &Texture, uv: Vec2| {
            let c = t.sample(uv);
            (c.x, c.y, c.z)
        };

        assert_eq!(rgb(got, Vec2::ZERO), (1.0, 0.0, 1.0), "не малиновая");
        assert_eq!(
            rgb(got, Vec2::new(0.9, 0.0)),
            rgb(&placeholder, Vec2::new(0.9, 0.0))
        );
    }

    #[test]
    fn try_load_texture_still_reports_the_error() {
        // Запасной выход для тех, кому отсутствующий файл — настоящий сбой,
        // а не «художник не дорисовал»
        let mut assets = Assets::new();

        assert!(
            assets
                .try_load_texture("такого-файла-нет.png", Magnify::Nearest, Minify::Nearest)
                .is_err()
        );
    }

    #[test]
    fn under_cargo_the_root_is_the_crate_being_run() {
        // Ветка «разработка»: cargo кладёт CARGO_MANIFEST_DIR в окружение и
        // при запуске тестов тоже, и указывает она на крейт, который cargo
        // запускает. Здесь это сам движок, у игры была бы игра.
        //
        // Ветку «поставка» так не проверить: она включается отсутствием
        // переменной, а убрать её у себя же нельзя — тесты и запущены cargo
        assert_eq!(
            Assets::new().asset_root(),
            Path::new(env!("CARGO_MANIFEST_DIR"))
        );
    }

    fn checker() -> Texture {
        Texture::checker(8, 2, [255, 255, 255, 255], [0, 0, 0, 255])
    }

    #[test]
    fn registering_builds_the_pyramid_for_whoever_asked_for_it() {
        // Ловушка, которую это и убирает: до переноса пирамиду надо было
        // строить руками, и «попросил Mipmapped, забыл построить» давало
        // тихий алиасинг вместо ошибки. Теперь забыть негде — намерение
        // объявлено фильтром, а память выделяет регистрация
        let mut assets = Assets::new();

        let mipped = assets.add_texture(checker().with_filter(Magnify::Nearest, Minify::Mipmapped));

        assert!(
            assets.texture(mipped).has_mipmaps(),
            "текстура просила мип-уровни и не получила их"
        );
    }

    #[test]
    fn a_texture_that_did_not_ask_pays_nothing() {
        // Обратная половина, и она не менее важна: пирамида стоит трети лишней
        // памяти, и раздавать её всем подряд незачем. Отладочной шахматке,
        // атласу интерфейса, таблице-данных она не нужна вовсе
        let mut assets = Assets::new();

        // По умолчанию Nearest/Nearest — сжатие мип-уровней не просит
        let plain = assets.add_texture(checker());
        let bilinear = assets.add_texture(checker().with_filter(Magnify::Linear, Minify::Linear));

        assert!(!assets.texture(plain).has_mipmaps());
        assert!(!assets.texture(bilinear).has_mipmaps());
    }
}
