# MiDeck

[github.com/ioseluiz/mi_deck](https://github.com/ioseluiz/mi_deck)

Widget de escritorio tipo Stream Deck para Windows. Un panel flotante de teclas que
abren aplicaciones, sitios web, carpetas y scripts con un clic. Funciona 100 % en
local: no usa red, no tiene cuenta y es inmune al proxy institucional.

Rust + Tauri 2. El frontend son módulos ES sin bundler ni `node_modules`.

## Estado

**Completo.** Fases 1 a 4: navegación con carpetas anidadas, las cuatro
acciones, imágenes propias por tecla, íconos extraídos de los `.exe`, bandeja del
sistema, instancia única, persistencia de posición, y edición completa desde la
interfaz: menú contextual, editor de teclas con vista previa en vivo, ajustes,
reordenar arrastrando y soltar archivos desde el Explorador, e instalador NSIS
que no pide permisos de administrador.

Editar `deck.json` a mano sigue siendo posible: `Ctrl+E` lo abre y `F5` lo recarga
sin reiniciar.

## Instalar

Descarga `MiDeck_<version>_x64-setup.exe` de la
[página de releases](https://github.com/ioseluiz/mi_deck/releases) y ejecútalo.

Se instala en `%LOCALAPPDATA%\MiDeck` y **no pide permisos de administrador**: el
desinstalador queda registrado en `HKCU`, no en la máquina. Requiere WebView2
Runtime, que viene de serie en Windows 11.

Medido en la versión de producción:

| | |
|---|---|
| Instalador | 2,2 MB |
| Ejecutable | 6,8 MB |
| Arranque en frío hasta ver la ventana | ~0,66 s |
| Memoria en reposo | ~30 MB el proceso, más ~126 MB de su WebView2 |

La memoria del WebView2 es el precio de usar un motor web para la interfaz; el
*working set* además sobreestima, porque buena parte se comparte con otros
procesos WebView2 del sistema.

## Compilar

```powershell
cargo tauri build
```

Deja el instalador en `<target>\release\bundle\nsis\`.

## Publicar

Al empujar una etiqueta de versión, GitHub Actions comprueba formato, clippy y
pruebas, compila el instalador y crea la release con el `.exe` adjunto:

```powershell
git tag v0.1.0
git push origin v0.1.0
```

El workflow sobrescribe `CARGO_TARGET_DIR`, porque `.cargo/config.toml` apunta a
una ruta local que en el runner no existe.

## Requisitos de desarrollo

Ya presentes en el equipo de desarrollo: Rust (toolchain `stable-x86_64-pc-windows-msvc`),
VS Build Tools con el componente C++, WebView2 Runtime y `tauri-cli`.

```powershell
cargo install tauri-cli --locked   # solo la primera vez
```

## Ejecutar

```powershell
cargo tauri dev
```

El directorio de compilación está fuera de OneDrive, en
`C:\Users\jlmunoz\.cargo-target\31_windows_widget`, configurado en `.cargo/config.toml`.
Es deliberado: `target/` son miles de archivos pequeños y sincronizarlos satura OneDrive.

## Comprobaciones

```powershell
python scripts/verificar_contraste.py   # contraste de la paleta
cargo test                      # 106 pruebas: store, integridad, lanzador, imágenes, íconos, edición
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Atajos

| Gesto | Efecto |
|---|---|
| Clic en una tecla | Ejecuta su acción |
| Clic en una tecla de carpeta | Entra al sub-deck |
| Tecla **Volver** (celda 0), `Backspace`, clic central, botón atrás del ratón | Sube un nivel |
| Clic en una miga de pan | Salta directo a ese nivel |
| Rueda sobre la rejilla, `Ctrl+Tab` | Cambia de página |
| `Ctrl+E` | Abre `deck.json` en el editor predeterminado |
| `F5` | Relee `deck.json` del disco sin reiniciar |
| Clic derecho sobre una tecla | Editar, pegar imagen, quitar imagen, duplicar, eliminar |
| Clic derecho sobre una celda vacía | Nueva tecla, nueva carpeta, añadir o quitar página, ajustes |
| Clic derecho sobre las migas | Ajustes, abrir `deck.json`, recargar |
| Arrastrar una tecla a otra celda | La mueve; si la celda está ocupada, intercambian |
| Arrastrar una tecla sobre una carpeta | La mete dentro |
| Soltar un `.exe`, carpeta o acceso directo | Crea una tecla en esa celda |
| Soltar una imagen sobre una tecla | Le cambia la cara, sin tocar su acción |
| `Ctrl+V` con el cursor sobre una tecla | Pega la imagen del portapapeles |
| Chincheta de la barra de título | Alterna entre "siempre encima" y "nivel escritorio" |
| Candado de la barra de título | Bloquea la posición: arrastrar ya no mueve el panel |
| `Ctrl+Alt+Espacio` | Trae el panel al frente desde cualquier sitio |
| Logo de GitHub en el pie | Abre el repositorio en el navegador |
| Arrastrar la barra de título | Mueve el panel |
| Cerrar con la X | Oculta a la bandeja; **no** sale. En nivel escritorio no oculta: manda el panel al fondo, donde vive |
| Volver a abrir el ejecutable | No arranca una segunda copia: trae al frente la que ya estaba |
| Clic izquierdo en el ícono de bandeja | Muestra u oculta el panel |
| Clic derecho en el ícono de bandeja | Menú: mostrar, siempre encima, abrir `deck.json`, iniciar con Windows, salir |

Cerrar no termina la aplicación: es un widget, no una ventana de documento. Para
salir de verdad, **Salir** en el menú de la bandeja.

## Contraste

La paleta no se eligió a ojo: se despejó a partir del contraste exigido. Los bordes
de las teclas y de las celdas vacías llegan a **3,16:1** sobre el panel, que supera
el mínimo de 3:1 que pide WCAG 1.4.11 para componentes de interfaz. Antes estaban
en **1,29:1** y sobre fondo oscuro una celda vacía era prácticamente invisible.

El contraste va en el **borde**, no en aclarar la cara de la tecla: así se mantiene
la estética de tecla oscura sobre cuerpo más oscuro. Y las celdas vacías llevan
trazo discontinuo, para que se distingan de una tecla llena por la forma y no solo
por el tono.

`python scripts/verificar_contraste.py` comprueba que una edición futura no baje
ninguno de esos valores, tanto en el panel como en la ventana del editor.

## Tamaño de la rejilla

Columnas, filas y tamaño de tecla se configuran en Ajustes. **La ventana se ajusta
sola** a lo que pida la rejilla: las medidas de la barra de título, el paginador y
el pie se leen del DOM en vez de estar codificadas, para que un cambio de CSS no
descuadre el cálculo. El resultado se limita al monitor actual, porque una ventana
sin bordes más grande que la pantalla no se puede ni mover ni cerrar.

## Dónde vive el panel

Tres niveles, en Ajustes o con la chincheta de la barra de título:

| Nivel | Comportamiento |
|---|---|
| **Siempre encima** (`top`) | Por encima de todas las ventanas. Siempre a mano, pero tapa lo que tengas debajo. |
| **Ventana normal** (`normal`) | Entra y sale del frente como cualquier otra ventana. |
| **Nivel escritorio** (`desktop`) | Sobre el fondo de pantalla, por debajo de todo. No estorba nunca: lo ves con `Win+D` o lo traes al frente con el atajo global. |

Todas las vías de "mostrar el panel" —el atajo, el icono de bandeja y volver a
lanzar el ejecutable— lo traen al frente de verdad. Sin esa excepción, en nivel
escritorio el manejador de foco lo devolvía al fondo en el mismo instante y
parecía que la aplicación no respondía.

Windows no tiene una bandera "siempre debajo" equivalente a `WS_EX_TOPMOST`, así que
el nivel escritorio se consigue empujando la ventana al fondo del orden z y
reimponiéndolo cada vez que recibe el foco. La excepción es cuando la trae el atajo
global: entonces se respeta delante hasta que vuelve a perder el foco, o el atajo la
mostraría y la escondería en el mismo instante.

En nivel escritorio, **pulsar el atajo otra vez la devuelve al fondo** en lugar de
ocultarla, porque su gracia ahí es seguir estando en su sitio cuando miras el
escritorio. En los otros niveles, la segunda pulsación sí la oculta.

El atajo se cambia en Ajustes, y dejándolo en blanco se desactiva. Si la combinación
ya la usa otra aplicación, se ignora en silencio: es un extra, no un motivo para que
el widget no arranque.

## Configuración

`%APPDATA%\MiDeck\deck.json`. Se puede redirigir con la variable de entorno
`DECK_CONFIG`, por ejemplo a una copia en OneDrive para compartirla entre equipos.

El guardado es atómico. Si el archivo se corrompe, se respalda como
`deck.json.bak-<AAAAMMDD-HHMMSS>` y el widget arranca con un deck por defecto: el
original nunca se pierde.

**Si lo editas con PowerShell**, cuidado con `ConvertTo-Json`: serializa una lista
de un solo elemento como el elemento suelto, sin corchetes. El formato lo tolera al
leer, pero es más seguro editarlo con un editor de texto o con Python.

### Estructura

Un registro plano de *superficies* con referencias, no un árbol anidado. Una
superficie es un nivel navegable —la raíz, o el interior de una carpeta— y un botón
de tipo `folder` apunta a otra superficie por id.

```json
{
  "version": 1,
  "settings": {
    "grid": { "cols": 5, "rows": 3 },
    "key_size": 96,
    "window_level": "top",
    "lock_position": false,
    "hotkey": "Ctrl+Alt+Space",
    "window": { "x": 1200, "y": 80 }
  },
  "root": "s-root",
  "surfaces": {
    "s-root": {
      "name": "Mi Deck",
      "pages": [
        {
          "buttons": [
            {
              "id": "b-outlook",
              "position": 0,
              "label": "Outlook",
              "icon": { "type": "auto" },
              "action": { "type": "app", "target": "C:\\...\\OUTLOOK.EXE", "args": "" }
            },
            {
              "id": "b-proy",
              "position": 1,
              "label": "Proyectos",
              "icon": { "type": "builtin", "name": "folder" },
              "action": { "type": "folder", "surface": "s-proy" }
            }
          ]
        }
      ]
    },
    "s-proy": { "name": "Proyectos", "pages": [{ "buttons": [] }] }
  }
}
```

`position` es el índice de celda (0-14 en una rejilla 5 × 3) y admite huecos. En una
superficie que no es la raíz, la celda 0 la ocupa la tecla **Volver**, que se inyecta
al pintar y no se guarda en el archivo.

### Tipos de acción

| `type` | Qué hace | Campos |
|---|---|---|
| `app` | Lanza un `.exe` o `.lnk` | `target`, `args`, `workdir`, `focus_if_running` |
| `url` | Abre una dirección web | `target`, `browser` (`default`, `edge`, `chrome`, `firefox` o ruta a un `.exe`), `profile` |
| `urls` | Abre varias direcciones de una vez | `targets` (lista), `browser`, `profile`, `new_window` |

Con `browser` distinto de `default`, el navegador se busca primero en el `PATH` y
después en `App Paths` del registro. Hace falta lo segundo: Edge y Chrome **no** se
añaden al `PATH`, y es por `App Paths` como Windows resuelve `Win+R → chrome`.
`ShellExecute` consulta esa clave sola, pero `CreateProcess` no, así que el
ejecutable se resuelve a su ruta completa antes de lanzarlo.
| `path` | Abre una carpeta, o revela un archivo seleccionado, en el Explorador | `target` |
| `script` | Ejecuta un script o comando | `shell` (`powershell`/`cmd`), `target`, `args`, `hidden` |
| `folder` | Navega a otra superficie del deck | `surface` |
| `hotkey` | Envía una combinación de teclas | `keys` |
| `text` | Teclea un texto literal | `text` |
| `system` | Acción del catálogo de Windows | `command` |

### Varias direcciones en una tecla

La acción `urls` abre una lista de direcciones de golpe, que es lo que casi
siempre se busca cuando alguien pide "abrir mi grupo de pestañas". Con Edge o
Chrome se lanza **una sola vez** con todas las direcciones, de modo que salen como
pestañas de la misma ventana; con `new_window` en una ventana nueva. Con el
navegador predeterminado se abren una a una, porque el shell solo acepta un
destino por llamada.

**No es un grupo de pestañas de Chrome.** Chrome no permite abrir un grupo
guardado desde fuera: no hay opción de línea de comandos ni esquema de URL para
eso, ni en MiDeck ni en ningún otro lanzador. Las pestañas salen sueltas, sin
etiqueta de color. Para un grupo de verdad, la única vía es un perfil de Chrome
por contexto y una acción `app` con `--profile-directory`.

El límite son 20 direcciones por tecla.

### Acciones de Windows

La acción `system` toma su `command` de un catálogo que vive en
`src-tauri/src/sistema.rs`. El editor construye su desplegable pidiéndoselo a Rust
con `list_system_commands`, así que añadir una acción es una línea en ese archivo
y no hay una segunda lista en JavaScript que se pueda quedar desfasada.

| Familia | `command` |
|---|---|
| Captura | `screenshot_region`, `screen_record` |
| Multimedia y volumen | `media_play_pause`, `media_next`, `media_prev`, `volume_up`, `volume_down`, `volume_mute` |
| Sistema | `lock`, `show_desktop`, `task_view`, `clipboard_history`, `emoji_picker`, `desktop_prev`, `desktop_next`, `file_explorer`, `task_manager`, `settings` |

Casi todas se llevan a cabo enviando una combinación, igual que `hotkey`. Dos no:
`settings` abre `ms-settings:` por el shell, y `lock` llama a `LockWorkStation` en
lugar de enviar `Win+L`, porque ese atajo se puede deshabilitar por directiva y en
un equipo gestionado eso es un riesgo real.

**Límite honesto de `screen_record`:** delega en la Barra de juegos de Xbox. No
graba el Explorador de archivos ni el escritorio, solo ventanas de aplicación, y si
está deshabilitada por directiva la tecla no hará nada. El editor lo advierte en el
propio formulario.

### Atajos y texto

`hotkey` envía la combinación a **la aplicación que tuvieras delante antes de pulsar
el panel**, no al panel: un gancho de Windows recuerda cuál era y le devuelve el
foco antes de enviar. Modificadores `Ctrl`, `Shift`, `Alt` y `Win`; teclas por
letra, dígito, `F1`–`F24` o nombre (`Esc`, `Tab`, `Supr`, `Intro`, `Inicio`,
`Izquierda`…), en español o en inglés. Una combinación mal escrita se detecta al
validar y marca la tecla en rojo, sin llegar a enviar nada.

`text` teclea carácter a carácter y admite tildes, `ñ` y emoji. **No lo uses para
contraseñas**: queda en claro en `deck.json`.

Cuidado con la pareja `path` y `folder`: en lenguaje coloquial ambas son "carpeta",
pero `path` abre el Explorador de Windows y `folder` navega dentro del propio deck.

En `target` y `args` se expanden las variables de Windows (`%USERPROFILE%`). Una
variable inexistente se deja visible a propósito, para que el mensaje de error muestre
qué falló en vez de dejar una ruta rota e inexplicable.

En `script`, si `target` termina en `.ps1` se ejecuta con `-File`; si no, se trata como
un comando suelto con `-Command`. Con `hidden: true` no aparece ninguna ventana.

Con `focus_if_running: true`, si ya hay una ventana de ese ejecutable abierta se
trae al frente en lugar de lanzar otra copia: pulsar "Outlook" diez veces no debe
dejar diez Outlooks.

### Tipos de ícono

| `type` | Campos | Nota |
|---|---|---|
| `image` | `file` | Nombre dentro de la biblioteca (ver abajo). |
| `auto` | — | Extrae el ícono de shell del destino. Para una acción `url` dibuja una pastilla con la inicial del dominio. |
| `builtin` | `name`: `folder`, `folder-open`, `globe`, `terminal`, `app`, `file`, `keyboard`, `text`, `pestanas`, `camara`, `video`, `play`, `siguiente`, `anterior`, `volumen-mas`, `volumen-menos`, `silencio`, `candado`, `escritorio`, `ventanas`, `portapapeles`, `emoji`, `escritorio-izq`, `escritorio-der`, `monitor`, `ajustes` | |
| `emoji` | `char` | |

Comunes a todos:

| Campo | Valores | Efecto |
|---|---|---|
| `fit` | `contain` (por defecto) · `cover` | `contain`: imagen centrada con la etiqueta debajo. `cover`: imagen a sangre cubriendo toda la cara de la tecla. |
| `label_style` | `below` · `overlay` · `none` | `overlay` pone la etiqueta sobre la imagen con un degradado al pie, para que se lea sobre cualquier foto. |
| `background` | HEX | Color de fondo de la tecla. |

### Biblioteca de imágenes

Las imágenes no se referencian por ruta absoluta: se importan a
`%APPDATA%\MiDeck\icons\` y se nombran por el hash de su contenido. Así importar
la misma imagen dos veces no la duplica, mover o borrar el archivo original no
rompe la tecla, y `deck.json` + la carpeta `icons\` son un paquete autocontenido
que se puede copiar a otro equipo.

Al importar se reescala al lado mayor de 256 px manteniendo la proporción, sin
recortar: el ajuste final lo hace `fit` en el CSS, de modo que cambiar de
`contain` a `cover` no obliga a reimportar. Los SVG se copian sin rasterizar. El
original se conserva en `icons\original\`.

Mientras no exista el arrastrar y soltar de la fase 3:

```powershell
cargo run --example importar_imagen -- "C:\ruta\logo.png"
```

Imprime el nombre que hay que poner en `"icon": { "type": "image", "file": "..." }`.

Al borrar un botón su imagen **no** se borra. Limpiarlas será una acción explícita
desde Ajustes (fase 3): un borrado automático convertiría un error de edición en
una pérdida de trabajo.

### Íconos automáticos

`"type": "auto"` extrae el ícono real de Windows con
`SHGetFileInfoW` + `SHGetImageList(SHIL_JUMBO)`, que da hasta 256 px y resuelve el
destino de los `.lnk`. Se descartó el crate `systemicons` justamente por lo
contrario: usa `ExtractIconExW`, tope 32 × 32 y sin resolver accesos directos.

El resultado se cachea en `%LOCALAPPDATA%\MiDeck\cache\icons\`, con clave de
ruta + fecha de modificación, así que actualizar una app refresca su ícono solo.
La caché se puede borrar sin consecuencias.

Para las URLs **no se descargan favicons**: se dibuja una pastilla con la inicial
del dominio sobre un color derivado del propio dominio. El widget no toca la red
jamás, lo que lo hace inmune al proxy institucional y a trabajar sin conexión. Si
quieres el logo real, impórtalo como imagen.

## Seguridad

`deck.json` es texto plano y las acciones de tipo `script` ejecutan lo que contengan
con los permisos del usuario. **No guardes credenciales ni tokens ahí.** Si un script
los necesita, que los lea de una variable de entorno o del Administrador de
credenciales de Windows.

## Estructura del proyecto

```
src/                    frontend: módulos ES, sin build
  index.html  styles.css
  api.js                envoltura de window.__TAURI__
  nav.js                pila de navegación y migas
  grid.js               pintado de la rejilla
  main.js               arranque y eventos
  menu.js               menú contextual
  dnd.js                arrastrar y soltar, y reordenar
  editor.html/.css/.js  ventana del editor y de ajustes
src-tauri/src/
  model.rs              structs serde del deck
  edit.rs               mutaciones puras: crear, mover, duplicar, borrar
  focus.rs              traer al frente una app ya abierta
  nivel.rs              nivel de ventana: normal, encima, escritorio
  screen.rs             validar la posicion contra los monitores conectados
  store.rs              deck.json: carga, guardado atómico, respaldo
  integrity.rs          referencias rotas, ciclos, superficies huérfanas
  launcher.rs           build_launch() puro + execute()
  icons.rs              ícono de shell vía API de Windows, con caché
  images.rs             biblioteca de imágenes: importar, normalizar, limpiar
  lib.rs                comandos Tauri, bandeja y arranque de la ventana
src-tauri/examples/
  importar_imagen.rs    importa una imagen desde la línea de comandos
scripts/generar_icono.py  regenera assets/icon.png (luego: cargo tauri icon)
```
