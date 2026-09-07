//! Загрузка Vulkan в рантайме — свой `dlopen`/`LoadLibrary`, без крейта
//! `libloading`.
//!
//! В отличие от, скажем, `libc` или `winit`, с Vulkan никогда не линкуются
//! напрямую (`#[link(name = "vulkan")]`): библиотеки на машине может не
//! быть вовсе — старое железо, машина без GPU-драйвера, — и тогда
//! приложение обязано упасть по своей собственной понятной проверке при
//! старте, а не по ошибке динамического линкера ещё до `main`. Поэтому
//! Vulkan всегда грузят вручную: `dlopen` библиотеки, `dlsym` за
//! `vkGetInstanceProcAddr`, а дальше уже сам `vkGetInstanceProcAddr`
//! достаёт все остальные функции — не только инструмента ради, так
//! устроен сам Vulkan: он единственная функция, которую гарантированно
//! экспортирует библиотека как символ, всё остальное — через неё.

use crate::vulkan::ffi::VkInstance;
use std::ffi::{CStr, CString, c_void};
use std::os::raw::c_char;

/// Указатель на функцию Vulkan без известной сигнатуры — то, что
/// возвращает `vkGetInstanceProcAddr`, прежде чем мы транзитом
/// (`mem::transmute`) приведём его к настоящему типу. Так делает и
/// оригинальный заголовок: `PFN_vkVoidFunction` там тоже нетипизирован
pub type PfnVoidFunction = unsafe extern "system" fn();
pub type PfnGetInstanceProcAddr =
    unsafe extern "system" fn(instance: VkInstance, name: *const c_char) -> Option<PfnVoidFunction>;

/// Достаёт функцию по имени через `vkGetInstanceProcAddr` и приводит её к
/// нужному типу. `$lib` — то, у чего есть метод `.get_instance(name)`;
/// `$name` — имя C-функции строкой; `$ty` — ожидаемый тип `PFN_*`.
///
/// Макрос, а не обычная функция, ровно по одной причине: `mem::transmute`
/// нужно знать целевой тип статически, а он у каждого вызова свой —
/// обычная функция такой типизации не даст без generics на каждый вызов
/// отдельно, что то же самое неудобство, только многословнее
#[macro_export]
macro_rules! vk_load {
    ($lib:expr, $instance:expr, $name:literal, $ty:ty) => {{
        let name = std::ffi::CString::new($name).unwrap();
        match $lib.get_instance($instance, &name) {
            Some(ptr) => unsafe { std::mem::transmute::<_, $ty>(ptr) },
            None => return Err(format!("Vulkan: функция {} не найдена", $name)),
        }
    }};
}

/// То же самое, что `vk_load!`, но для функций уровня device — их не
/// достать через `vkGetInstanceProcAddr`, только через
/// `vkGetDeviceProcAddr`. Не только ради типов: у устройств с несколькими
/// установленными ICD (например, dGPU + iGPU одновременно) он даёт указатель
/// сразу на реализацию конкретного драйвера, минуя уровень диспетчеризации
/// instance — тот самый «быстрый путь», которым официально рекомендуют
/// пользоваться для всего, что принимает `VkDevice`/`VkQueue`/`VkCommandBuffer`
#[macro_export]
macro_rules! vk_load_device {
    ($get_device_proc_addr:expr, $device:expr, $name:literal, $ty:ty) => {{
        let name = std::ffi::CString::new($name).unwrap();
        match unsafe { $get_device_proc_addr($device, name.as_ptr()) } {
            Some(ptr) => unsafe { std::mem::transmute::<_, $ty>(ptr) },
            None => return Err(format!("Vulkan: функция {} не найдена", $name)),
        }
    }};
}

/// Платформенный `dlopen`/`LoadLibrary` — единственное место в модуле,
/// разведённое по ОС веткой `cfg`, всё остальное (`Library`, макрос выше)
/// общее для всех трёх
#[cfg(unix)]
mod platform {
    use std::os::raw::{c_char, c_int, c_void};

    // На Linux libdl исторически отдельная библиотека (в новых glibc она
    // слита в libc, но линковать явно безопаснее — так работает при любой
    // версии glibc). На macOS dlopen/dlsym — часть libSystem, куда линкуется
    // любой бинарник по умолчанию, отдельно просить не нужно
    #[cfg_attr(target_os = "linux", link(name = "dl"))]
    unsafe extern "C" {
        fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn dlclose(handle: *mut c_void) -> c_int;
    }

    /// RTLD_NOW: разрешить все символы сразу, при загрузке, а не лениво при
    /// первом обращении. Ошибку в отсутствующей библиотеке лучше поймать
    /// здесь, одним `dlopen`, чем потом на случайном dlsym посреди кадра
    const RTLD_NOW: c_int = 2;

    pub unsafe fn open(path: *const c_char) -> *mut c_void {
        unsafe { dlopen(path, RTLD_NOW) }
    }
    pub unsafe fn sym(handle: *mut c_void, name: *const c_char) -> *mut c_void {
        unsafe { dlsym(handle, name) }
    }
    pub unsafe fn close(handle: *mut c_void) {
        unsafe {
            dlclose(handle);
        }
    }

    #[cfg(target_os = "macos")]
    /// Пробуем по очереди: сначала настоящий Vulkan Loader (если
    /// установлен Vulkan SDK для macOS или `brew install vulkan-loader`),
    /// потом MoltenVK напрямую — так делают многие инди-приложения на Mac,
    /// когда ставить полный SDK ради одной библиотеки не хочется. Loader
    /// предпочтительнее: именно он умеет найти MoltenVK как ICD через
    /// `VK_ICD_FILENAMES`, слои валидации и вообще всё остальное, что
    /// полагается настоящему Vulkan-окружению.
    ///
    /// Абсолютные пути Homebrew — не для красоты: `brew install molten-vk`
    /// кладёт `libMoltenVK.dylib` в `/opt/homebrew/lib` (Apple Silicon) или
    /// `/usr/local/lib` (Intel), а bare-имя `dlopen` там САМО не найдёт —
    /// в отличие от `/usr/lib`, эти каталоги не входят в путь поиска
    /// системного загрузчика по умолчанию. Без явного пути пришлось бы
    /// вручную выставлять `DYLD_LIBRARY_PATH` перед каждым запуском —
    /// именно так это и обнаружилось при первом прогоне `vulkan_triangle`
    pub const CANDIDATES: &[&str] = &[
        "libvulkan.dylib",
        "libvulkan.1.dylib",
        "libMoltenVK.dylib",
        "/opt/homebrew/lib/libMoltenVK.dylib",
        "/usr/local/lib/libMoltenVK.dylib",
    ];

    #[cfg(target_os = "linux")]
    pub const CANDIDATES: &[&str] = &["libvulkan.so.1", "libvulkan.so"];
}

#[cfg(windows)]
mod platform {
    use std::os::raw::{c_char, c_void};

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryA(filename: *const c_char) -> *mut c_void;
        fn GetProcAddress(handle: *mut c_void, name: *const c_char) -> *mut c_void;
        fn FreeLibrary(handle: *mut c_void) -> i32;
    }

    pub unsafe fn open(path: *const c_char) -> *mut c_void {
        unsafe { LoadLibraryA(path) }
    }
    pub unsafe fn sym(handle: *mut c_void, name: *const c_char) -> *mut c_void {
        unsafe { GetProcAddress(handle, name) }
    }
    pub unsafe fn close(handle: *mut c_void) {
        unsafe {
            FreeLibrary(handle);
        }
    }

    pub const CANDIDATES: &[&str] = &["vulkan-1.dll"];
}

/// Загруженная библиотека Vulkan плюс единственная функция, ради которой
/// её вообще грузили — `vkGetInstanceProcAddr`, из которой достаётся всё
/// остальное (см. doc-комментарий модуля)
pub struct Library {
    handle: *mut c_void,
    get_instance_proc_addr: PfnGetInstanceProcAddr,
}

impl Library {
    /// Перебирает кандидатов имени библиотеки для текущей ОС (см.
    /// `platform::CANDIDATES`) и берёт первого, кто откроется
    pub fn load() -> Result<Self, String> {
        let mut handle = std::ptr::null_mut();
        let mut opened_name = "";

        for candidate in platform::CANDIDATES {
            let cname = CString::new(*candidate).unwrap();
            let h = unsafe { platform::open(cname.as_ptr()) };
            if !h.is_null() {
                handle = h;
                opened_name = candidate;
                break;
            }
        }

        if handle.is_null() {
            return Err(format!(
                "не удалось найти Vulkan ни под одним из имён {:?} — установлен ли Vulkan SDK / MoltenVK?",
                platform::CANDIDATES
            ));
        }
        eprintln!("Vulkan: загружен {opened_name}");

        let proc_addr_name = CString::new("vkGetInstanceProcAddr").unwrap();
        let symbol = unsafe { platform::sym(handle, proc_addr_name.as_ptr()) };
        if symbol.is_null() {
            unsafe {
                platform::close(handle);
            }
            return Err("библиотека найдена, но в ней нет vkGetInstanceProcAddr — это не Vulkan".into());
        }

        // transmute указателя данных в указатель на функцию — единственный
        // способ получить вызываемую функцию из dlsym/GetProcAddress: они
        // сами по себе возвращают void*, а не типизированный fn-указатель
        let get_instance_proc_addr: PfnGetInstanceProcAddr = unsafe { std::mem::transmute(symbol) };

        Ok(Self { handle, get_instance_proc_addr })
    }

    /// Достаёт функцию через `vkGetInstanceProcAddr`. `instance` можно
    /// передать `VkInstance::NULL` — тогда доступны только те немногие
    /// функции, что не привязаны к конкретному instance
    /// (`vkCreateInstance`, `vkEnumerateInstance*Properties`); все
    /// остальные требуют настоящий, уже созданный `VkInstance`
    pub fn get_instance(&self, instance: VkInstance, name: &CStr) -> Option<PfnVoidFunction> {
        unsafe { (self.get_instance_proc_addr)(instance, name.as_ptr()) }
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        unsafe {
            platform::close(self.handle);
        }
    }
}
