//! Платформенная поверхность окна — единственное место во всём модуле, где
//! приходится по-настоящему развести код по ОС: у Vulkan нет своего понятия
//! «окно», только голый `VkSurfaceKHR`, а получить его можно исключительно
//! через платформенное расширение, которое понимает конкретный оконный
//! сервер (Metal-слой на Mac, HWND на Windows, Xlib-окно на Linux).
//!
//! Самый неочевидный кусок — Mac. `raw-window-handle` отдаёт с окна
//! `NSView*`, а `VK_EXT_metal_surface` (расширение, которым MoltenVK
//! подключает Vulkan к Metal) хочет `CAMetalLayer*`. Между ними разница в
//! том, что вид (`NSView`) — это виджет AppKit, а слой (`CALayer`) — это
//! то, чем реально владеет Core Animation и что реально показывает
//! Metal-контент; своего слоя у свежесозданного `NSView` может и не быть,
//! его нужно завести и подставить руками. Без крейта `objc`/`cocoa` это
//! значит звать Objective-C рантайм напрямую — `objc_msgSend` — тот же
//! путь, которым устроены сами эти крейты внутри

use crate::vulkan::ffi::*;
use crate::vulkan::instance::Instance;
use crate::vulkan::loader::{Library, vk_load};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawWindowHandle};
// Нужен только ветке Xlib — без `cfg` это предупреждение о неиспользуемом
// импорте на всех остальных платформах
#[cfg(target_os = "linux")]
use raw_window_handle::RawDisplayHandle;
use std::ffi::{CStr, c_void};

/// Платформенное расширение, которое нужно запросить у `Instance::new` ещё
/// до его создания — `VK_KHR_surface` запрашивается всегда вместе с ним
pub fn platform_extension() -> &'static CStr {
    #[cfg(target_os = "macos")]
    {
        c"VK_EXT_metal_surface"
    }
    #[cfg(target_os = "windows")]
    {
        c"VK_KHR_win32_surface"
    }
    #[cfg(target_os = "linux")]
    {
        c"VK_KHR_xlib_surface"
    }
}

/// `VK_KHR_surface` — общая часть, нужна на всех платформах вдобавок к
/// платформенному расширению выше
pub const SURFACE_EXTENSION: &CStr = c"VK_KHR_surface";

/// `scale_factor` — отношение физических пикселей к логическим у этого окна
/// (`winit::window::Window::scale_factor`). Нужен только на Mac и только
/// затем, чтобы поверхность не оказалась вдвое мельче окна на Retina —
/// подробности у `attach_metal_layer`. На Windows/Linux размер поверхности
/// в пикселях спрашивается у оконной системы напрямую, и параметр там не
/// участвует
#[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
pub fn create<W>(lib: &Library, instance: &Instance, window: &W, scale_factor: f64) -> Result<VkSurfaceKHR, String>
where
    W: HasWindowHandle + HasDisplayHandle,
{
    let window_handle = window
        .window_handle()
        .map_err(|e| format!("не удалось достать нативный хендл окна: {e}"))?;

    match window_handle.as_raw() {
        #[cfg(target_os = "macos")]
        RawWindowHandle::AppKit(handle) => create_metal(lib, instance, handle.ns_view.as_ptr(), scale_factor),

        #[cfg(target_os = "windows")]
        RawWindowHandle::Win32(handle) => create_win32(lib, instance, handle),

        #[cfg(target_os = "linux")]
        RawWindowHandle::Xlib(handle) => {
            let display_handle = window
                .display_handle()
                .map_err(|e| format!("не удалось достать хендл дисплея: {e}"))?;
            match display_handle.as_raw() {
                RawDisplayHandle::Xlib(display) => create_xlib(lib, instance, handle, display),
                _ => Err("окно на X11, но хендл дисплея не Xlib (Wayland?) — пока не поддержано".into()),
            }
        }

        _ => Err("этот тип окна на данной платформе не поддержан (нужен AppKit/Win32/Xlib)".into()),
    }
}

// ============================================================================
// macOS — VK_EXT_metal_surface поверх CAMetalLayer
// ============================================================================

#[cfg(target_os = "macos")]
#[repr(C)]
struct VkMetalSurfaceCreateInfoEXT {
    s_type: VkEnum,
    p_next: *const c_void,
    flags: VkFlags,
    /// `const CAMetalLayer*` — непрозрачный для нас указатель, мы его не
    /// разыменовываем, только передаём дальше в MoltenVK
    p_layer: *const c_void,
}

#[cfg(target_os = "macos")]
type PfnCreateMetalSurfaceEXT = unsafe extern "system" fn(
    VkInstance,
    *const VkMetalSurfaceCreateInfoEXT,
    *const c_void,
    *mut VkSurfaceKHR,
) -> VkEnum;

/// Голый Objective-C рантайм — три функции, которых достаточно, чтобы
/// послать любое сообщение любому объекту без крейта `objc`
#[cfg(target_os = "macos")]
mod objc {
    use std::ffi::CStr;
    use std::os::raw::c_char;
    use std::os::raw::c_void;

    #[link(name = "objc")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> *mut c_void;
        fn sel_registerName(name: *const c_char) -> *mut c_void;
        /// В заголовке `objc_msgSend` объявлена variadic — но реальный ABI
        /// вызова определяется числом и типами аргументов НА МЕСТЕ вызова,
        /// а не декларацией функции. Поэтому берём один и тот же символ и
        /// `transmute`-им его к разным сигнатурам под конкретные вызовы —
        /// ровно так это делает и крейт `objc` внутри своего `msg_send!`
        fn objc_msgSend();
    }

    pub fn class(name: &CStr) -> *mut c_void {
        unsafe { objc_getClass(name.as_ptr()) }
    }

    pub fn sel(name: &CStr) -> *mut c_void {
        unsafe { sel_registerName(name.as_ptr()) }
    }

    /// `[receiver sel]` — сообщение без аргументов, например `alloc`/`init`
    pub unsafe fn send0(receiver: *mut c_void, selector: *mut c_void) -> *mut c_void {
        let f: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
            unsafe { std::mem::transmute(objc_msgSend as *const c_void) };
        unsafe { f(receiver, selector) }
    }

    /// `[receiver sel: arg]` с указателем-аргументом, например `setLayer:`
    pub unsafe fn send_ptr(receiver: *mut c_void, selector: *mut c_void, arg: *mut c_void) -> *mut c_void {
        let f: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> *mut c_void =
            unsafe { std::mem::transmute(objc_msgSend as *const c_void) };
        unsafe { f(receiver, selector, arg) }
    }

    /// `[receiver sel: BOOL]`, например `setWantsLayer:`. `BOOL` в
    /// современном (64-битном) Objective-C рантайме — это ровно `bool`
    /// (один байт), не `int`, — перепутать легко, если помнить старый
    /// 32-битный ABI, где `BOOL` был `signed char` иного размера, чем сам
    /// `bool`, хотя оба варианта укладываются в один байт
    pub unsafe fn send_bool(receiver: *mut c_void, selector: *mut c_void, arg: bool) -> *mut c_void {
        let f: unsafe extern "C" fn(*mut c_void, *mut c_void, bool) -> *mut c_void =
            unsafe { std::mem::transmute(objc_msgSend as *const c_void) };
        unsafe { f(receiver, selector, arg) }
    }

    /// `[receiver sel: CGFloat]`, например `setContentsScale:`. `CGFloat` на
    /// 64-битных платформах — это `f64`, и объявить аргумент правильно здесь
    /// важнее, чем в остальных случаях: целые аргументы едут в обычных
    /// регистрах, а вещественные — в векторных, поэтому ошибка в типе не
    /// «слегка исказит число», а передаст мусор из совсем другого регистра
    pub unsafe fn send_f64(receiver: *mut c_void, selector: *mut c_void, arg: f64) -> *mut c_void {
        let f: unsafe extern "C" fn(*mut c_void, *mut c_void, f64) -> *mut c_void =
            unsafe { std::mem::transmute(objc_msgSend as *const c_void) };
        unsafe { f(receiver, selector, arg) }
    }
}

/// Заводит `CAMetalLayer` и подставляет его слоем указанному `NSView`.
///
/// `alloc`+`init`, а не удобный конструктор вроде `+layer`: `alloc`
/// возвращает объект с retain count 1 без пула авторелизов, и владение
/// сразу понятно — тот же паттерн, каким пользуется любой Objective-C код.
///
/// **И эту единицу надо отдать обратно.** В Objective-C нет заимствований,
/// владение считается вручную: `alloc` даёт нам +1, а `setLayer:` берёт
/// СВОЙ +1 (`NSView.layer` — strong-свойство), итого 2. Мы дальше слой не
/// держим — только передаём указатель в `VkMetalSurfaceCreateInfoEXT` и
/// забываем, — значит свою единицу обязаны вернуть `release`. Без него слой
/// не освободится никогда: вид отпустит свою ссылку при уничтожении окна, а
/// наша так и останется. Утечка при этом ровно на один объект за всё время
/// жизни процесса — потому и не бросалась в глаза.
///
/// Отпускаем ПОСЛЕ `setLayer:`, и порядок здесь ровно тот, что делает
/// операцию безопасной: к моменту `release` сильную ссылку уже держит вид,
/// так что счётчик падает с 2 до 1, а не с 1 до 0. Сделать наоборот —
/// отпустить до передачи виду — значит освободить слой прямо себе под
/// ногами.
///
/// **`contentsScale` обязателен, и это не мелочь.** Слой, заведённый руками,
/// получает `contentsScale = 1.0`, и его `drawableSize` остаётся в
/// ЛОГИЧЕСКИХ пикселях. Свой слой AppKit за нас не поправит — он обновляет
/// масштаб только у слоёв, которые вид завёл себе сам. MoltenVK отдаёт
/// `drawableSize` как `currentExtent` поверхности, `choose_extent` берёт
/// его как есть (поверхность диктует размер — так и задумано), и на Retina
/// получается swapchain ровно вдвое мельче окна по каждой оси: картинка
/// рисуется в 800×600 и растягивается компоновщиком на окно 1600×1200.
///
/// Ошибка предельно тихая: ничего не падает, ничего не искажается, просто
/// кадр мягче, чем должен быть, — а списать это легко на что угодно, от
/// отсутствия сглаживания до самой шахматки. Замерено пробой: до правки
/// «окно 1600x1200 физ., swapchain extent 800x600», после — совпадают.
/// Это тот же самый разрыв между логическими и физическими пикселями, что
/// уже описан для CPU-пути в CLAUDE.md («Размер кадра — не константа»)
#[cfg(target_os = "macos")]
fn attach_metal_layer(ns_view: *mut c_void, scale_factor: f64) -> *mut c_void {
    use objc::*;
    unsafe {
        let layer_class = class(c"CAMetalLayer");
        let alloc = send0(layer_class, sel(c"alloc"));
        let layer = send0(alloc, sel(c"init"));

        // Порядок важен: сначала объявляем вид layer-backed, потом
        // подставляем СВОЙ слой вместо того, что вид завёл бы себе сам
        send_bool(ns_view, sel(c"setWantsLayer:"), true);
        send_ptr(ns_view, sel(c"setLayer:"), layer);

        // После подстановки, а не до: так масштаб точно не потеряется, что
        // бы AppKit ни делал со слоем в момент привязки к виду
        send_f64(layer, sel(c"setContentsScale:"), scale_factor);

        // Отдаём свой +1 от `alloc`: дальше слоем владеет вид (см.
        // doc-комментарий). Возвращаемый указатель после этого — заимствование,
        // а не владение, и живёт он ровно столько же, сколько окно, — то есть
        // дольше, чем поверхность, которую из него делают
        send0(layer, sel(c"release"));

        layer
    }
}

#[cfg(target_os = "macos")]
fn create_metal(
    lib: &Library,
    instance: &Instance,
    ns_view: *mut c_void,
    scale_factor: f64,
) -> Result<VkSurfaceKHR, String> {
    let create_metal_surface_ext: PfnCreateMetalSurfaceEXT =
        vk_load!(lib, instance.handle, "vkCreateMetalSurfaceEXT", PfnCreateMetalSurfaceEXT);

    let layer = attach_metal_layer(ns_view, scale_factor);

    let create_info = VkMetalSurfaceCreateInfoEXT {
        s_type: VK_STRUCTURE_TYPE_METAL_SURFACE_CREATE_INFO_EXT,
        p_next: std::ptr::null(),
        flags: 0,
        p_layer: layer as *const c_void,
    };

    let mut surface = VkSurfaceKHR::NULL;
    let result =
        unsafe { create_metal_surface_ext(instance.handle, &create_info, std::ptr::null(), &mut surface) };
    if result != VK_SUCCESS {
        return Err(format!("vkCreateMetalSurfaceEXT вернул {result}"));
    }
    Ok(surface)
}

// ============================================================================
// Windows — VK_KHR_win32_surface
// ============================================================================

#[cfg(target_os = "windows")]
#[repr(C)]
struct VkWin32SurfaceCreateInfoKHR {
    s_type: VkEnum,
    p_next: *const c_void,
    flags: VkFlags,
    hinstance: *mut c_void,
    hwnd: *mut c_void,
}

#[cfg(target_os = "windows")]
type PfnCreateWin32SurfaceKHR = unsafe extern "system" fn(
    VkInstance,
    *const VkWin32SurfaceCreateInfoKHR,
    *const c_void,
    *mut VkSurfaceKHR,
) -> VkEnum;

#[cfg(target_os = "windows")]
fn create_win32(
    lib: &Library,
    instance: &Instance,
    handle: raw_window_handle::Win32WindowHandle,
) -> Result<VkSurfaceKHR, String> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleA(module_name: *const std::os::raw::c_char) -> *mut c_void;
    }

    let create_win32_surface_khr: PfnCreateWin32SurfaceKHR =
        vk_load!(lib, instance.handle, "vkCreateWin32SurfaceKHR", PfnCreateWin32SurfaceKHR);

    let hinstance = unsafe { GetModuleHandleA(std::ptr::null()) };
    let hwnd = handle.hwnd.get() as *mut c_void;

    let create_info =
        VkWin32SurfaceCreateInfoKHR { s_type: VK_STRUCTURE_TYPE_WIN32_SURFACE_CREATE_INFO_KHR, p_next: std::ptr::null(), flags: 0, hinstance, hwnd };

    let mut surface = VkSurfaceKHR::NULL;
    let result =
        unsafe { create_win32_surface_khr(instance.handle, &create_info, std::ptr::null(), &mut surface) };
    if result != VK_SUCCESS {
        return Err(format!("vkCreateWin32SurfaceKHR вернул {result}"));
    }
    Ok(surface)
}

// ============================================================================
// Linux — VK_KHR_xlib_surface
// ============================================================================

#[cfg(target_os = "linux")]
#[repr(C)]
struct VkXlibSurfaceCreateInfoKHR {
    s_type: VkEnum,
    p_next: *const c_void,
    flags: VkFlags,
    dpy: *mut c_void,
    window: std::os::raw::c_ulong,
}

#[cfg(target_os = "linux")]
type PfnCreateXlibSurfaceKHR = unsafe extern "system" fn(
    VkInstance,
    *const VkXlibSurfaceCreateInfoKHR,
    *const c_void,
    *mut VkSurfaceKHR,
) -> VkEnum;

#[cfg(target_os = "linux")]
fn create_xlib(
    lib: &Library,
    instance: &Instance,
    window: raw_window_handle::XlibWindowHandle,
    display: raw_window_handle::XlibDisplayHandle,
) -> Result<VkSurfaceKHR, String> {
    let create_xlib_surface_khr: PfnCreateXlibSurfaceKHR =
        vk_load!(lib, instance.handle, "vkCreateXlibSurfaceKHR", PfnCreateXlibSurfaceKHR);

    let dpy = display
        .display
        .map(|p| p.as_ptr())
        .ok_or_else(|| "у окна нет хендла X11-дисплея".to_string())?;

    let create_info = VkXlibSurfaceCreateInfoKHR {
        s_type: VK_STRUCTURE_TYPE_XLIB_SURFACE_CREATE_INFO_KHR,
        p_next: std::ptr::null(),
        flags: 0,
        dpy,
        window: window.window,
    };

    let mut surface = VkSurfaceKHR::NULL;
    let result =
        unsafe { create_xlib_surface_khr(instance.handle, &create_info, std::ptr::null(), &mut surface) };
    if result != VK_SUCCESS {
        return Err(format!("vkCreateXlibSurfaceKHR вернул {result}"));
    }
    Ok(surface)
}
