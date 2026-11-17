# Diseño: Rustpira — recordatorio de descansos para Windows

Fecha: 2026-11-16
Estado: borrador para aprobación

## 1. Intención

Rustpira es una aplicación residente para Windows 10/11 que recuerda —y, si se configura, obliga— a tomar un descanso cada N minutos mientras se trabaja en la computadora. Está inspirada en BreakTimer, pero con dos motivaciones concretas:

- **Consumo de memoria bajo.** BreakTimer está hecho en Electron y consume mucha RAM para una tarea tan simple. Rustpira debe ser liviana: es el requisito número 1.
- **Disciplina configurable.** BreakTimer no permite limitar cuántas veces se puede cancelar un descanso. Rustpira agrega límites de cancelación y posposición, y un bloqueo duro que no se puede esquivar desde la propia aplicación.

Usuario principal: el autor, para uso personal. El código se publica en un repositorio público y se comparte libremente.

## 2. Objetivos, no objetivos y anti-objetivos

**Objetivos**
- Residir en el área de notificaciones con un consumo de RAM mínimo.
- Avisar del descanso según intervalo, horarios de trabajo e inactividad.
- Bloquear la pantalla (todos los monitores) durante el descanso, con reglas de cancelación y posposición configurables.
- Interfaz y documentación completamente en español.

**No objetivos**
- Estadísticas, historial o reportes de descansos.
- Sincronización entre equipos, cuentas de usuario o servidor.
- Soporte para macOS o Linux.
- Protección de la configuración con contraseña (la usa el mismo usuario que se autoimpone la disciplina).
- Instalador (se distribuye como `.exe` portable).

**Anti-objetivos** (cosas que harían fracasar el proyecto aunque funcione)
- Que consuma RAM comparable a una app Electron.
- Que el bloqueo duro se pueda esquivar desde la propia app (menú, configuración, teclado habitual) **mientras está en pantalla**.
- Que interrumpa fuera del horario de trabajo o al volver de una ausencia larga.

## 3. Restricciones

- Plataforma: Windows 10 y 11, x86_64, toolchain `stable-x86_64-pc-windows-msvc`. Proyecto Rust edición 2024 (ya creado con `cargo new`).
- Interfaz gráfica: **Slint** (decisión del usuario).
- **Límite de Windows aceptado por el usuario:** ninguna aplicación de usuario puede bloquear Ctrl+Alt+Supr ni la pantalla segura de Windows. Desde ahí se puede abrir el Administrador de tareas y cerrar la app. El "bloqueo duro" es tan duro como Windows lo permite, igual que en BreakTimer.
- Idioma: toda la interfaz (textos, menús, tooltips, mensajes de error) y la documentación (README, docs) en español. *Decisión del diseñador:* identificadores y comentarios del código en inglés, según la convención habitual en Rust.
- Distribución: un único `.exe` portable, sin instalador ni permisos de administrador. Licencia MIT (*decisión del diseñador*).
- Ícono: lo provee el usuario en `recursos/rustpira.png` (PNG de 1254×1254 con transparencia, 1,4 MB). Ese original se conserva como fuente y no se incrusta. Las versiones que usa la app ya están generadas:
  - `recursos/rustpira.ico`: multitamaño (16, 20, 24, 32, 48 y 256 px), 119 KB.
  - `recursos/rustpira-256.png`: 256×256, 105 KB.

## 4. Enfoques considerados

| Enfoque | RAM estimada | Esfuerzo | Tema claro/oscuro | Resultado |
|---|---|---|---|---|
| **Slint** + crates nativos de Windows | baja (~15–30 MB) | medio; interfaz en lenguaje `.slint` | incluido (estilo Fluent) | **Elegido** |
| egui/eframe + crates nativos | media (~40–70 MB con ventana) | bajo; todo en Rust | incluido | Descartado por RAM y estilo poco nativo |
| Win32 puro (`windows` / `native-windows-gui`) | mínima (~5–10 MB) | muy alto | modo oscuro deficiente en controles clásicos | Descartado por esfuerzo |
| Tauri / Electron | alta (80–150 MB) | bajo | sí | Descartado de entrada: contradice el requisito 1 |

Las cifras de RAM son estimaciones, no mediciones. El usuario eligió Slint sin hacer una prueba previa. Separar la vista (`.slint`) de la lógica (Rust) además favorece una arquitectura limpia.

## 5. Funcionalidad

### 5.1 Ícono en el área de notificaciones

- La app arranca sin ventanas, solo con el ícono.
- **Ícono:** se genera a partir de `recursos/rustpira.png` y se usa en tres lugares: la bandeja, el propio `.exe` (lo que se ve en el Explorador) y las ventanas.
  - `rustpira.ico` se incrusta en el `.exe` con `build.rs` y se usa en la bandeja y en las ventanas. En la bandeja se carga el tamaño que pide Windows según el DPI.
  - `rustpira-256.png` se incrusta para mostrarlo dentro de la pantalla de descanso y en Acerca de.
- **Tooltip** (se actualiza cada segundo), según el estado:
  - `Siguiente descanso en hh:mm:ss`
  - `Recordatorios desactivados hasta las HH:MM`
  - `Fuera del horario de trabajo`
  - `En pausa por inactividad`
  - `Descanso en curso`
- **Menú contextual** (clic derecho):
  ```
  Configuración
  Desactivar recordatorios ▶ 15 min / 30 min / 1 h / 2 h / 4 h
  Reactivar ahora              (solo visible si están desactivados)
  ─────────────
  Acerca de
  Salir
  ```
- **Mientras hay un descanso en pantalla** (normal o duro), quedan deshabilitados "Configuración", "Desactivar recordatorios" y "Salir". "Acerca de" sigue disponible. Deshabilitar "Desactivar" lo pidió el usuario. Extenderlo a "Configuración" y "Salir" es *decisión del diseñador*, para que el bloqueo duro no se esquive por ahí.
- **Desactivar por un lapso:** pausa el temporizador. Al vencer el lapso, o al elegir "Reactivar ahora", el contador arranca desde cero (*decisión del diseñador*).
- **Una sola instancia:** si se ejecuta de nuevo, la segunda instancia abre la configuración de la que ya está corriendo y termina (*decisión del diseñador*).

### 5.2 Temporizador

- Cuenta los minutos configurados (intervalo) hasta el próximo descanso.
- Solo cuenta **dentro de los horarios de trabajo** del día actual (sección 5.5). Fuera de horario está en pausa. Al empezar cada franja, el contador arranca desde cero (*decisión del diseñador*). Si una franja termina en medio de un descanso, el descanso se completa igual.
- **Inactividad** (requisito del usuario):
  - Se puede activar o desactivar, con un umbral de N minutos configurable.
  - Si está activa y se cumple el umbral sin actividad de teclado o mouse, el contador **deja de contar**.
  - Cuando se detecta actividad otra vez, **el contador empieza desde el inicio**.
- **Suspensión, hibernación y bloqueo de sesión de Windows:** al volver, el contador arranca desde cero, esté o no activa la detección de inactividad (*decisión del diseñador*).
- **Si eso ocurre en medio de un descanso**, la cuenta regresiva sigue con la hora real, porque el tiempo fuera cuenta como descanso. Al volver, si el descanso ya terminó, se cierra. Si no, continúa con el tiempo que le queda (*decisión del diseñador*).
- Si se cambia el intervalo en la configuración, el contador se reinicia al guardar.

### 5.3 El descanso

**Tipo de aviso** (configurable):

1. **Solo notificación:** una notificación nativa de Windows con el título y el mensaje configurados, más el sonido. No bloquea, no tiene duración ni botones, y el ciclo se reinicia en el momento en que se muestra. Las reglas de cancelación y posposición no se aplican.
2. **Bloqueo de pantalla completa:**
   - Una ventana sin bordes, siempre visible por encima de todo, en **cada monitor**.
   - El fondo es **un color o una imagen** (PNG/JPG) elegidos por el usuario, con **una única transparencia configurable** que se aplica a cualquiera de los dos.
   - La imagen se escala para cubrir la pantalla sin deformarse, recortando lo que sobre (*decisión del diseñador*). Se carga al empezar el descanso y se libera al terminar.
   - En el monitor principal se muestran, de arriba hacia abajo: **el ícono de la app** (requisito del usuario; el tamaño exacto lo decide el implementador, alrededor de 96–128 px), el título, el mensaje de N líneas, la cuenta regresiva `mm:ss` y los botones que correspondan.
   - Si el aviso empieza con una aplicación a pantalla completa en primer plano (juego, presentación, video), se muestra igual, sin esperar a que se cierre (*decisión del diseñador*: es una app de disciplina).
   - Mientras dura, bloquea el acceso a todo lo que esté abierto: captura mouse y teclado, y suprime Alt+Tab, la tecla Windows, Alt+F4 y Esc.
   - Al llegar la cuenta a 0, se cierra sola. Opcionalmente suena un aviso de fin (*decisión del diseñador*).

**Duración del descanso:** configurable en minutos. *Decisión del diseñador:* hacía falta para la cuenta regresiva y para el bloqueo duro, y no estaba en la lista original.

### 5.4 Reglas de cancelación y posposición

**Cancelar** (saltea el descanso y reinicia el ciclo completo):
- `Permitir cancelar`: sí / no. Si es "no", el botón no aparece nunca.
- `Limitar cancelaciones seguidas`: sí / no, con un valor N.
- Cada cancelación suma 1 al contador de cancelaciones seguidas.
- **Bloqueo duro:** al alcanzar N, el próximo descanso se muestra sin ningún botón (ni Cancelar ni Posponer) y no se libera hasta que termina la cuenta regresiva.
- El contador de cancelaciones seguidas vuelve a 0 cada vez que se completa un descanso entero, sea normal o duro.

*Interpretación del diseñador:* activar el límite implica activar el bloqueo duro al alcanzarlo. Sin límite, se puede cancelar sin restricción.

**Alcance del bloqueo duro (decisión del usuario):** es duro **solo mientras está en pantalla**. Entre descansos, la configuración y el menú funcionan con normalidad aunque haya un bloqueo duro pendiente: se puede cambiar el tipo de aviso, el límite o N, usar "Desactivar recordatorios" o salir de la app. Si la app se cierra a la fuerza en medio de un descanso (por ejemplo, desde el Administrador de tareas), al volver a abrirla no se retoma ese descanso. Estas salidas están aceptadas y deben quedar documentadas en el README.

**Posponer** (requisito del usuario):
- Botón `Posponer M min`, con M configurable.
- Se puede posponer hasta K veces, con K configurable de **0 a 3**. Con K = 0 el botón no aparece.
- *Interpretación del diseñador:* K es **por descanso**. Al posponer, la pantalla se cierra y vuelve a aparecer a los M minutos. Agotadas las K veces, el botón desaparece en ese descanso. Posponer no suma al contador de cancelaciones.
- En el bloqueo duro no se puede posponer.

### 5.5 Horarios de trabajo

- Cada día de la semana tiene **una lista de franjas horarias** (`HH:MM–HH:MM`), con botones para agregar y quitar.
- Un día sin franjas significa que no hay avisos ese día.
- Hay un botón "Copiar a todos los días hábiles" (lunes a viernes) para no cargar lo mismo cinco veces (*decisión del diseñador*).
- Validación: el inicio debe ser anterior al fin y las franjas de un mismo día no pueden superponerse. Las franjas no cruzan la medianoche.

### 5.6 Sonido

- Se puede elegir entre 3 o 4 sonidos incluidos en el ejecutable o un `.wav` propio (*decisión del diseñador*).
- Volumen de 0 a 100, con botón "Probar".
- Suena al empezar el descanso, tanto en notificación como en bloqueo.
- Los sonidos incluidos deben tener una licencia compatible con MIT (CC0 o de creación propia).

### 5.7 Ventana de configuración

Organizada en pestañas (*decisión del diseñador*):

| Pestaña | Contenido |
|---|---|
| General | intervalo (min), duración del descanso (min), tipo de aviso, iniciar con Windows |
| Mensaje | título de la ventana de descanso, mensaje de N líneas |
| Horarios | franjas por día de la semana |
| Descanso | permitir cancelar, límite de cancelaciones seguidas, posponer (M min, hasta K veces), inactividad (activar, N min) |
| Apariencia | fondo: color o imagen; transparencia; tema de la ventana: Claro / Oscuro |
| Sonido | sonido, volumen, probar |

- Botones `Guardar` y `Cancelar`. Los cambios se aplican al guardar.
- **Tema Claro / Oscuro:** se aplica a la ventana de configuración y a "Acerca de".
- **Iniciar con Windows:** la app se registra en `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, sin permisos de administrador.
- La ventana se crea al abrirla y se destruye al cerrarla, para liberar memoria.

### 5.8 Acerca de

Una ventana pequeña con el ícono de la app, el nombre, la versión, la licencia, un enlace al repositorio y la mención a Slint que exige su licencia gratuita.

### 5.9 Persistencia

- La configuración se guarda en `%APPDATA%\rustpira\config.toml`, en un formato legible y editable a mano.
- Si el archivo no existe o es inválido, se usan los valores por defecto. Si era inválido, se guarda una copia como `config.toml.bak` y se avisa con una notificación.
- *Decisión del diseñador:* el contador de cancelaciones seguidas también se persiste, para que reiniciar la app no sea una forma de esquivar el bloqueo duro.

### 5.10 Valores por defecto (*decisión del diseñador*)

Intervalo de 50 min. Descanso de 5 min. Bloqueo de pantalla completa. Cancelar permitido, sin límite. Posponer 5 min, hasta 1 vez. Inactividad activa a los 5 min. Horario lunes a viernes de 09:00 a 18:00. Fondo de color oscuro con 80 % de opacidad. Tema claro. Primer sonido incluido al 50 %. No inicia con Windows.

## 6. Arquitectura propuesta

Arquitectura hexagonal liviana:

- **Dominio** (Rust puro, sin dependencias de Windows ni de Slint): la máquina de estados del temporizador (activo, en pausa por horario, en pausa por inactividad, desactivado hasta, descanso en curso, pospuesto), las reglas de horarios, de cancelación y de posposición, y el cálculo del texto del tooltip. Recibe el reloj y la inactividad como entradas inyectadas, así se puede testear de forma determinista con `cargo test`.
- **Adaptadores de Windows:** el ícono de la bandeja (`tray-icon`), la inactividad (`GetLastInputInfo`), los eventos de suspensión y bloqueo de sesión, el registro de arranque, el gancho de teclado para el bloqueo, las notificaciones nativas, el sonido con volumen, la instancia única y la enumeración de monitores.
- **Interfaz (Slint):** la configuración, el overlay de descanso y Acerca de. Las ventanas se crean solo cuando se necesitan.
- **Persistencia:** `serde` + `toml`.
- **Recursos de build:** `recursos/rustpira.ico`, incrustado en el `.exe` con un script `build.rs`.

La elección concreta de crates para el sonido y las notificaciones queda a cargo del implementador, con la restricción de priorizar la RAM.

## 7. Criterios de éxito

- RAM en reposo (solo el ícono, sin ventanas): **< 20 MB** de working set privado. *Objetivo propuesto por el diseñador*; se mide con el Administrador de tareas.
- RAM durante un descanso con imagen de fondo en dos monitores: < 60 MB. Al terminar el descanso vuelve al valor de reposo.
- Durante un bloqueo duro no hay forma de volver al escritorio desde la app ni con los atajos habituales. Solo queda Ctrl+Alt+Supr, que es el límite aceptado.
- Toda la lógica del dominio está cubierta por tests deterministas.

## 8. Riesgos técnicos a verificar al principio

1. Que Slint pueda crear ventanas sin bordes, transparentes y siempre visibles por encima de todo en cada monitor, y que sus fondos admitan transparencia.
2. Que el event loop de Slint pueda seguir corriendo sin ventanas abiertas y convivir con `tray-icon`.
3. Que la memoria se libere de verdad al cerrar las ventanas de Slint.
4. Que el tema claro/oscuro se pueda cambiar en tiempo de ejecución.
5. Los términos exactos de la licencia gratuita de Slint para aplicaciones de escritorio, y su compatibilidad con publicar el código bajo MIT.
6. La forma en que el gancho de teclado y la captura del foco conviven con aplicaciones a pantalla completa (juegos, presentaciones).

**Plan de respaldo** (*decisión del diseñador*):
- Los riesgos 1 a 4 se validan en una prueba técnica al principio, antes de construir el resto.
- Si Slint no resuelve bien el overlay, se permite un híbrido: el overlay de descanso en Win32 nativo (ventanas *layered*) y Slint solo para Configuración y Acerca de.
- Se puede usar el renderizador por software de Slint si gasta menos memoria que los acelerados.
- Las cifras de RAM de la sección 7 son **objetivos a medir y reportar**, no condiciones para aceptar o rechazar. El requisito real es "mucho menos que una app Electron".
- Licencia (riesgo 5): el código propio va bajo MIT, y el ejecutable distribuido cumple la licencia gratuita de Slint con la mención en Acerca de. Si eso resultara incompatible, se consulta al usuario.

## 9. Qué queda a criterio del implementador

- La estructura de módulos y crates dentro de la arquitectura de la sección 6.
- La elección de crates para el sonido, las notificaciones y la instancia única.
- El diseño visual de las ventanas, dentro del estilo Fluent de Slint y del tema elegido.
- El formato exacto de las claves de `config.toml`.
