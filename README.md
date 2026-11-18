# Rustpira

Rustpira le recuerda que tome descansos mientras trabaja en la computadora. Funciona en Windows 10 y 11, vive en el área de notificación (junto al reloj) y, cuando llega la hora, cubre todos los monitores con una pantalla de descanso o muestra una notificación. Está escrita en Rust y Slint para usar muy poca memoria, es un único ejecutable portable y no necesita permisos de administrador.

## Inicio rápido

1. Descargue `rustpira.exe` y guárdelo en una carpeta propia, por ejemplo `Documentos\Rustpira`.
2. Ejecútelo. Aparece el ícono de Rustpira en el área de notificación; si no lo ve, búsquelo en la flecha de íconos ocultos.
3. Haga clic derecho en el ícono y elija **Configuración** para ajustar intervalos, horarios y aspecto. Pase el mouse sobre el ícono para ver cuánto falta para el próximo descanso.

Para que arranque con Windows, active **Iniciar con Windows** en la pestaña General.

## Qué hace

| Función | Detalle |
|---|---|
| Descansos | Cada cierto tiempo de trabajo (por defecto, 33 minutos) muestra un descanso de unos minutos (por defecto, 5). |
| Tipo de aviso | Pantalla completa en todos los monitores, o solo una notificación de Windows con sonido. |
| Horarios | Franjas de trabajo por día de la semana; fuera de ellas no hay avisos. Cada día se puede deshabilitar sin borrar sus franjas. |
| Inactividad | Si no usa el teclado ni el mouse durante unos minutos, el contador se detiene; al volver, empieza de cero y se avisa con una notificación. |
| Cancelar y posponer | Se puede permitir cancelar el descanso, limitar las cancelaciones seguidas y posponerlo unos minutos, hasta 3 veces. |
| Descanso obligatorio | Al llegar al límite de cancelaciones, el siguiente descanso se muestra sin botones hasta que termina. |
| Aspecto | Fondo de color o una imagen propia (PNG o JPG), con opacidad ajustable. Tema claro u oscuro para las ventanas. |
| Sonido | Cuatro sonidos incluidos o un archivo `.wav` propio, con volumen. |

## Menú del ícono

| Opción | Qué hace |
|---|---|
| Configuración | Abre la ventana de configuración. Los cambios se aplican al guardar y el contador vuelve a empezar. |
| Desactivar recordatorios | Pausa los avisos durante 15 minutos, 30 minutos, 1, 2 o 4 horas. |
| Reactivar ahora | Aparece mientras los avisos están desactivados y los vuelve a activar. |
| Acerca de | Versión, licencia, datos del desarrollador y enlace al repositorio. |
| Salir | Cierra Rustpira. |

Mientras hay un descanso en pantalla, **Configuración**, **Desactivar recordatorios** y **Salir** quedan deshabilitados.

## Límites del descanso obligatorio

Durante un descanso, la pantalla cubre todos los monitores y se bloquean los atajos para salir de ella (Alt+Tab, Alt+F4, Esc). El descanso obligatorio además oculta los botones. No es un bloqueo de seguridad: estas salidas siguen disponibles a propósito o porque Windows no permite bloquearlas:

- **Ctrl+Alt+Supr**: Windows lo reserva para el sistema y ninguna aplicación lo puede bloquear.
- **Tecla Windows**: el menú Inicio se abre por encima de la pantalla de descanso.
- **Administrador de tareas**: permite cerrar Rustpira como cualquier otro programa.
- **Entre descansos**: la configuración y la opción Salir están disponibles con normalidad.

El bloqueo dura solo mientras el descanso está en pantalla.

## Archivos

Todo se guarda en su perfil de usuario, en `%APPDATA%\rustpira\`:

| Archivo | Contenido |
|---|---|
| `config.toml` | Sus preferencias. Se puede editar a mano con Rustpira cerrado. |
| `config.toml.bak` | Copia del archivo anterior si estaba dañado o tenía valores inválidos; en ese caso Rustpira usa los valores por defecto y avisa. |
| `state.toml` | El contador de cancelaciones seguidas, para que reiniciar la aplicación no lo borre. |

**Iniciar con Windows** agrega el valor `Rustpira` en `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, solo para su usuario. Si mueve el ejecutable, Rustpira actualiza la ruta la próxima vez que lo abra.

Para desinstalar: desactive **Iniciar con Windows**, cierre Rustpira y borre el ejecutable y la carpeta `%APPDATA%\rustpira\`.

## Notificaciones

El modo "Solo notificación" y el aviso al volver de la inactividad usan las notificaciones de Windows. Si no las ve, revise que estén activadas en **Configuración de Windows > Sistema > Notificaciones** y que el modo "No molestar" esté apagado.

## Compilar desde el código

Requisitos: Windows 10 u 11 y [Rust](https://rustup.rs/) estable con el destino MSVC (`stable-x86_64-pc-windows-msvc`).

| Qué | Comando |
|---|---|
| Compilar y ejecutar (depuración, con consola para los mensajes) | `cargo run` |
| Tests | `cargo test` |
| Lint | `cargo clippy --all-targets -- -D warnings` |
| Formato | `cargo fmt --check` |
| Ejecutable final | `cargo build --release` → `target\release\rustpira.exe` |

La guía para contribuir (arquitectura y reglas del proyecto) está en [`CLAUDE.md`](CLAUDE.md) y el diseño en [`docs/superpowers/specs/`](docs/superpowers/specs/).

## Licencia

Rustpira es software libre bajo la [licencia MIT](LICENSE). Desarrollado por [Sistemas Torres](https://www.sistemastorres.com/).

La interfaz está hecha con [Slint](https://slint.dev/), que se usa bajo su licencia gratuita (Royalty-free License 2.0). Esa licencia pide mostrar esta insignia en la página donde se descarga la aplicación:

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev/)
