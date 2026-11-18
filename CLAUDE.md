# Rustpira — guía del proyecto

Aplicación residente para Windows 10/11 que recuerda (y opcionalmente obliga) a tomar descansos. Rust + Slint. Prioridad número 1: **bajo consumo de RAM**.

- Diseño aprobado: `docs/superpowers/specs/2026-11-16-rustpira-design.md`
- Seguimiento de la feature actual: `odd/tasks/rustpira-mvp.md`

## Reglas que no se negocian

1. **Idioma.** Toda la interfaz (textos, menús, tooltips, mensajes de error) y toda la documentación van en español neutro. Los identificadores, comentarios de código y mensajes de commit van en inglés.
2. **RAM primero.** Antes de sumar una dependencia, evaluar su costo en memoria y tamaño del binario. Las ventanas se crean solo cuando se necesitan y se destruyen al cerrarse. Los recursos pesados (imagen de fondo) se cargan durante el descanso y se liberan al terminar.
3. **Dominio puro.** La lógica (temporizador, horarios, reglas de cancelación y posposición, textos del tooltip) no depende de Windows ni de Slint. Recibe el reloj y la inactividad como entradas, así se puede testear de forma determinista.
4. **Test primero.** Cada regla de comportamiento arranca con un test que falla (`cargo test`) y después se implementa.
5. **Sin permisos de administrador.** Todo lo que toque el sistema (arranque con Windows, configuración) usa el perfil del usuario: `HKCU` y `%APPDATA%\rustpira\`.

## Arquitectura

```
src/
  domain/      lógica pura: config, timer, reglas de descanso, tooltip
  platform/    adaptadores de Windows: bandeja, inactividad, energía/sesión,
               registro, gancho de teclado, sonido, notificaciones
  storage/     archivos en %APPDATA%\rustpira\: config.toml (preferencias, con .bak)
               y state.toml (contador de cancelaciones); escritura atómica
  audio.rs     sonidos incluidos (sintetizados) y volumen de un .wav; puro, sin Windows
  break_screen.rs  pantalla de descanso: una ventana por monitor + bloqueo de teclado
  about.rs     ventana "Acerca de" (versión y repositorio salen de Cargo.toml)
  settings/    ventana de configuración: form.rs (datos del formulario, puro) y window.rs (Slint)
  ui/          integración con Slint (ventanas)
ui/            archivos .slint (vistas)
examples/      pruebas técnicas ejecutables (spike_slint.rs: bandeja, overlay, RAM)
recursos/      íconos (el PNG original es la fuente; no se incrusta)
```

Las dependencias van hacia adentro: `ui`, `platform` y `storage` usan `domain`; `domain` no usa a ninguno. Los módulos se declaran en `src/lib.rs`; `src/main.rs` solo arma la app.

Renderizador de Slint: **solo `renderer-software`**. femtovg/OpenGL se midió en 529 MB y no libera memoria (ver `odd/tasks/rustpira-mvp.md`, L4).

## Comandos

| Qué | Comando |
|---|---|
| Compilar y correr (debug) | `cargo run` |
| Tests | `cargo test` |
| Lint (sin warnings) | `cargo clippy --all-targets -- -D warnings` |
| Formato | `cargo fmt --check` |
| Ejecutable final | `cargo build --release` → `target\release\rustpira.exe` |

Antes de cada commit: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` y `cargo test` en verde.

## Commits

Conventional Commits en inglés (`feat:`, `fix:`, `test:`, `docs:`, `chore:`), un commit por unidad de trabajo, con sus tests y su documentación incluidos. Sin líneas de atribución a IA.
