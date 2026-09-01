//! Софтверный 3D-растеризатор.
//!
//! Крейт собирается и как библиотека (`xd_engine`), и как исполняемый файл:
//! `main.rs` — просто один из её потребителей. Благодаря этому движок доступен
//! интеграционным тестам в `tests/`, примерам в `examples/` и doc-тестам.

pub mod app;
pub mod clipping;
pub mod config;
pub mod fps_counter;
pub mod math;
pub mod renderer;
pub mod scene;
pub mod texture;
