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
| Botón **menú** de la barra de título | Nueva tecla, nueva carpeta, páginas, ajustes, abrir `deck.json`, recargar |
| Botón **ancla** de la barra de título | Solo aparece con un perfil activo: lo fija o lo suelta |
| Clic derecho sobre una tecla | Editar, pegar imagen, quitar imagen, duplicar, eliminar, ajustes |
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
| `macro` | Varias acciones en orden, con pausas | `steps` |

### Elegir la aplicacion sin saber su ruta

La acción `app` ofrece primero un desplegable con las aplicaciones del **menú
Inicio**, que es lo que el usuario ya reconoce, y debajo deja el campo de ruta
intacto para quien quiera apuntar a un `.exe` concreto. Elegir de la lista
escribe la ruta en ese campo: lo que se guarda sigue siendo una ruta, visible y
editable.

La lista sale de `src-tauri/src/apps.rs`, que recorre el menú Inicio de la
máquina y el del usuario buscando `.lnk` y `.url`. Se descartan las entradas de
desinstalación, ayuda y documentación, que nadie quiere en una tecla.

Se eligió el menú Inicio y no el registro de desinstalación porque ese enumera
paquetes y no aplicaciones: salen actualizaciones y redistribuibles, y la mitad
no tiene con qué lanzarse. **Limitación conocida:** las aplicaciones de la Tienda
que no dejan acceso directo en el menú Inicio no aparecen; para esas está el
campo de ruta.

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
| Captura | `screenshot_region`, `screenshot_full`, `screenshot_window`, `screen_record` |
| Multimedia y volumen | `media_play_pause`, `media_next`, `media_prev`, `volume_up`, `volume_down`, `volume_mute` |
| Sistema | `lock`, `show_desktop`, `task_view`, `clipboard_history`, `emoji_picker`, `desktop_prev`, `desktop_next`, `file_explorer`, `task_manager`, `settings` |
| Energía ⚠ | `sleep`, `sign_out`, `restart`, `shutdown`, `empty_recycle_bin` |

Casi todas se llevan a cabo enviando una combinación, igual que `hotkey`. Dos no:
`settings` abre `ms-settings:` por el shell, y `lock` llama a `LockWorkStation` en
lugar de enviar `Win+L`, porque ese atajo se puede deshabilitar por directiva y en
un equipo gestionado eso es un riesgo real.

**Límite honesto de `screen_record`:** delega en la Barra de juegos de Xbox. No
graba el Explorador de archivos ni el escritorio, solo ventanas de aplicación, y si
está deshabilitada por directiva la tecla no hará nada. El editor lo advierte en el
propio formulario.

### Acciones destructivas y doble confirmación

Las de la familia **Energía** no se ejecutan al primer clic. La primera
pulsación **arma** la tecla: se pone roja y su etiqueta pasa a «¿Seguro?». La
segunda ejecuta. Se desarma sola a los 3 segundos, al pulsar cualquier otra tecla
o al sacar el cursor de la rejilla.

Quién decide qué es peligroso es el catálogo de Rust, no el frontend: `DeckView`
trae `confirm_required` con los ids de botón afectados y el JavaScript solo
consulta la pertenencia, igual que con las referencias rotas. Así una acción
destructiva nueva queda protegida por el hecho de declararla, sin que haya que
acordarse de tocar también el JavaScript. Un test comprueba que la
correspondencia entre «peligrosa» y la familia Energía sea exacta **en los dos
sentidos**: marcar peligrosa una acción inofensiva molesta, pero no marcar una
destructiva apaga el equipo de alguien.

El color no va solo: la etiqueta cambia, porque quien no distingue el rojo
también tiene que enterarse de que esa pulsación todavía no ha hecho nada.

Dos detalles de la implementación:

- **Apagar y reiniciar necesitan `SeShutdownPrivilege`**, que un proceso tiene
  concedido pero *deshabilitado* de nacimiento. Sin activarlo con
  `AdjustTokenPrivileges`, `ExitWindowsEx` falla con «no se tienen los privilegios
  necesarios» y la tecla parece rota sin motivo aparente.
- **No se usa `EWX_FORCE`**, solo `EWX_FORCEIFHUNG`. Una tecla no tiene por qué
  tirar por la borda el trabajo sin guardar de nadie: Windows pide a las
  aplicaciones que cierren y avisa si alguna lo impide.

Bloquear el equipo **no** pide confirmación: es reversible con la contraseña y
pedir confirmación para algo inofensivo enseña a confirmar sin leer.

### Capturas de pantalla

`screenshot_full` y `screenshot_window` las hace MiDeck, no Windows. Guardan un
PNG en `Imágenes\MiDeck` —o donde digan los ajustes— y, si así se configura, lo
dejan además en el portapapeles. La tecla avisa de la ruta al terminar: una
captura que no dice dónde quedó es una captura perdida.

Dos detalles que no son obvios:

- **El panel se aparta de la foto.** Capturar "toda la pantalla" y que salga el
  botón que acabas de pulsar no es lo que nadie espera, así que la ventana se
  oculta, se espera a que el compositor repinte y se vuelve a mostrar con su
  nivel original.
- **BitBlt no escribe el canal alfa.** Deja basura, casi siempre ceros, de modo
  que el PNG sale entero transparente y la captura parece vacía. Hay que forzarlo
  a opaco, y hay un test que lo vigila.

La ventana activa se captura con `PrintWindow` y `PW_RENDERFULLCONTENT`, que pide
a la ventana que se dibuje ella misma: sale entera aunque el panel la estuviera
tapando. Después se recorta el borde invisible de redimensión que `GetWindowRect`
incluye y DWM no, para que no quede un cerco negro alrededor. La ventana que se
captura es **la que estaba delante antes de pulsar el panel**, no el panel.

La captura de **región** se queda delegada en `Win+Shift+S`: exige una
superposición de selección que Windows ya trae resuelta.

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
`SHGetFileInfoW` + `SHGetImageList(SHIL_JUMBO)`, que da hasta 256 px. Se descartó
el crate `systemicons` por lo contrario: usa `ExtractIconExW`, tope 32 × 32.

**Los accesos directos se resuelven antes por COM** (`IShellLinkW`): se usa el
ícono que el propio `.lnk` declara y, si no declara ninguno, el del programa al
que apunta. Se creyó que `SHGetFileInfoW` ya lo hacía solo, y no: un `.lnk` daba
el ícono de documento en blanco aunque su destino tuviera el suyo. Con la lista
de aplicaciones del menú Inicio, que es toda accesos directos, ese caso dejó de
ser raro para ser el normal.

El resultado se cachea en `%LOCALAPPDATA%\MiDeck\cache\icons\`, con clave de
ruta + fecha de modificación, así que actualizar una app refresca su ícono solo.
La caché se puede borrar sin consecuencias.

Para las URLs **no se descargan favicons**: se dibuja una pastilla con la inicial
del dominio sobre un color derivado del propio dominio. El widget no toca la red
jamás, lo que lo hace inmune al proxy institucional y a trabajar sin conexión. Si
quieres el logo real, impórtalo como imagen.

### Un tipo de acción que esta versión no conoce

Si `deck.json` trae una acción de una versión más nueva, **no se da el archivo
por corrupto**. Esa tecla se conserva tal cual bajo `{"type": "unknown",
"__original": {…}}`, sale en el panel con un triángulo de aviso y, al pulsarla,
dice de qué tipo es y que hay que actualizar. Al volver a una versión que sí lo
entienda, la tecla se recupera sola.

Hace falta porque sin ello una sola tecla incomprensible invalidaba el archivo
entero: se respaldaba y se arrancaba de cero, y el usuario veía que había perdido
las quince. **Esto protege a partir de la v0.2.0**: las versiones anteriores ya
publicadas no lo llevan, así que bajar de la v0.2.0 a la v0.1.2 con teclas de tipo
`urls` o `system` sigue siendo destructivo.

## La rueda del ratón sobre una tecla

Girar la rueda encima de una tecla puede subir y bajar algo sin pulsar nada. Es el
dial de una consola, en software — y es lo único de esta lista que un Stream Deck
no puede hacer sin comprarle el modelo con ruedas.

Se activa por tecla, en el editor, con una acción por sentido:

```json
{ "wheel": {
  "up":   { "type": "system", "command": "volume_up" },
  "down": { "type": "system", "command": "volume_down" }
} }
```

Las teclas con rueda llevan una **marca `↕` en la esquina**: una rueda que no se ve
no la usa nadie.

### El reparto con el paginador

La rueda sobre la rejilla ya cambiaba de página, y eso se conserva: sobre una tecla
**con** rueda la gira, y sobre cualquier otra cosa cambia de página. El reparto vive
dentro del mismo manejador a propósito — con dos manejadores, el de la tecla no
podría evitar que el de la página se disparara también, y girar el volumen cambiaría
de página a la vez.

### Por qué se cuenta el delta y no los eventos

Una vuelta de rueda son decenas de eventos en medio segundo. Se agrupan en una sola
llamada cada 60 ms con un contador de muescas, **hasta diez por tanda**: cada muesca
es un proceso o una entrada sintética, y más de eso no se nota en pantalla pero sí en
la máquina.

El contador suma `deltaY` y lo divide por la unidad más pequeña del gesto, en vez de
contar eventos. Contar eventos sería más simple pero solo vale para una rueda: un
panel táctil de precisión manda muchos eventos de delta pequeño en lugar de muescas
enteras, y contarlos uno a uno dispararía la acción decenas de veces por un gesto
corto.

Medido con un archivo por muesca: una muesca da una, cinco seguidas a 25 ms dan
cinco, y diez a 8 ms dan diez. Ni una perdida.

## Macros

Una tecla, varios pasos en orden. Es lo que hace falta para automatizar una
función de una aplicación: rara vez es *una* pulsación.

```json
{ "type": "macro", "steps": [
  { "action": { "type": "hotkey", "keys": "Ctrl+L" }, "delay_ms": 200 },
  { "action": { "type": "text",   "text": "cmd"    }, "delay_ms": 80  },
  { "action": { "type": "hotkey", "keys": "Intro"  } }
] }
```

Esa macro abre una consola en la carpeta que tengas delante en el Explorador, que
es la secuencia que mucha gente hace a mano. **La pausa es después de cada paso**,
que es como se piensa al escribirla: «manda `Ctrl+L`, espera a que la barra tome
el foco, escribe `cmd`». Así tampoco hace falta un tipo de paso «esperar»: la
espera cuelga del paso anterior.

Un paso puede ser **cualquier acción de MiDeck**, incluida una captura de
pantalla. Eso sale gratis de que el ejecutor viva en `run_action` y no en
`launcher::execute`: hay un solo sitio que sabe ejecutar cualquier cosa.

### Lo que se rechaza, y por qué

| Límite | Motivo |
|---|---|
| Máximo 20 pasos | Una macro de cincuenta no es una tecla, es un script, y para eso está la acción `script` |
| Pausas ≤ 10 s en total | Mientras corre, la tecla está ocupada; un tope evita que una errata deje el deck pensando medio minuto |
| Una macro dentro de otra | Dos macros que se llamen entre sí serían recursión infinita |
| Un paso que entra a una carpeta | Navegar a mitad de secuencia deja el panel donde nadie pidió |

### Nada se ejecuta a medias

**La secuencia entera se valida antes del primer paso.** Hizo falta ponerlo
explícitamente y lo encontró un test: `execute` valida solo el lanzamiento que
recibe, así que sin esa comprobación previa una errata en el paso tres se habría
descubierto con los dos primeros ya ejecutados. Una macro a medias es peor que una
que no arranca.

El mensaje dice **qué paso** falla: «Paso 2: No se reconoce la tecla “Inventada”».

### Límite del editor

Cada fila del editor ofrece el campo principal de su tipo. El modelo admite
cualquier acción completa en un paso, pero un formulario con todos los campos de
cada tipo dentro de cada fila sería ilegible; quien necesite argumentos o un
perfil de navegador puede escribirlo en `deck.json`, que lo acepta igual.

Y lo que la fila no enseña **sobrevive a abrir la macro y guardarla**: el editor
parte de la acción que había y solo pisa el campo que esa fila edita. Antes la
reconstruía desde cero, así que una carpeta de trabajo escrita a mano se perdía al
primer paso por el editor, que es la peor forma de perderla: sin tocarla.

**Una macro teclea sobre lo que tenga el foco.** Si una pausa se queda corta, lo
que escriba acaba en otro sitio. El editor lo advierte.

## La burbuja flotante

Un botón redondo de 56 px en una esquina, siempre encima de todo: un clic trae el
panel y otro lo esconde. Se enciende en **Ajustes → Burbuja flotante**, donde
también se elige la esquina.

Existe por el nivel *escritorio*: ahí el panel vive bajo todo lo demás y traerlo
exige acordarse del atajo global o encontrar el icono en la bandeja. Es la misma
raíz del reporte de «se cierra y no puedo volver a abrirla»: el panel no se
cerraba, se escondía donde nadie sabía buscarlo.

### No roba el foco, y eso no era gratis

Si al pulsarla Windows le quitara el foco a la aplicación que tienes delante, se
rompería todo lo de la fase 5: `devolver_foco()` no tendría a dónde volver y los
atajos llegarían al sitio equivocado.

Lo resuelve `WS_EX_NOACTIVATE`, aplicado a mano después de crear la ventana porque
Tauri no lo expone. Era el supuesto sin verificar de todo el plan —nada garantizaba
que WebView2 siguiera entregando los clics del ratón con ese estilo— así que la
fase empezó por medirlo antes de dibujar nada. **Funciona**: con una consola
delante, el clic llega al webview y el primer plano sigue siendo la consola.

### Los tres detalles que la hacen usable

| Detalle | Por qué |
|---|---|
| `WS_POPUP`, sin menú de sistema | Medido aquí: una ventana que lo conserva **no baja de 136 px de ancho** por mucho que se le pida 56, porque Windows le reserva sitio a unos botones que no existen |
| `SetWindowRgn` elíptica | La ventana es cuadrada y el botón redondo: sin recorte, las cuatro esquinas transparentes se comerían los clics de lo que haya debajo |
| `SPI_GETWORKAREA` y píxeles físicos | La posición se calcula contra el **área de trabajo**, no contra la pantalla, así que nunca queda debajo de la barra de tareas, esté donde esté |

`WS_EX_TOOLWINDOW` además la saca del Alt+Tab, donde un botón de 56 px no pinta
nada. Y al capturar toda la pantalla se aparta junto con el panel: si no, saldría
en todas y cada una de las capturas.

### Lo que cuesta, medido

**Unos 65 MB**: en compilación de release, 460,5 MB con ella y 395,7 MB sin ella,
y un proceso más de WebView2. En compilación de desarrollo salen +70 MB, así que
la cifra es consistente.

Es bastante más de lo que se estimó al planificarla, y por eso **viene apagada de
fábrica** y al apagarla la ventana se destruye en vez de esconderse: un WebView2
escondido sigue costando lo mismo. Si algún día molesta, la alternativa es
reescribirla como ventana Win32 por capas, que no gasta casi nada pero son unas
300 líneas de código inseguro sin poder reaprovechar nada del frontend.

## `%CARPETA%`: la carpeta que tienes delante

Escribe `%CARPETA%` en una tecla y se sustituye por la ruta de la ventana del
Explorador que tuvieras delante al pulsarla.

| Tecla | Acción | Campo |
|---|---|---|
| Consola aquí | `app` | `cmd.exe`, carpeta de trabajo `%CARPETA%` |
| PowerShell aquí | `app` | `powershell.exe`, carpeta de trabajo `%CARPETA%` |
| Abrir en VS Code | `app` | argumentos `"%CARPETA%"` |
| Copiar la ruta | `text` | `%CARPETA%` |
| Script sobre esta carpeta | `script` | argumentos `-Ruta "%CARPETA%"` |

Una pieza, muchas teclas. Entrecomíllala cuando vaya en argumentos: sin comillas,
«Mis documentos» se parte en dos.

### Por qué no basta con una macro

La macro `Ctrl+L` → `cmd` → `Intro` abre la consola en esa carpeta, pero lo hace a
ciegas: MiDeck no sabe dónde está, solo aporrea el teclado. Con la ventana
equivocada delante esas tres letras acaban escritas en un documento —en Word,
`Ctrl+L` alinea el párrafo a la izquierda y lo demás se escribe tal cual— y si la
pausa se queda corta, también.

`%CARPETA%` pregunta la ruta y la recibe como dato. **No teclea nada.** Si no hay
un Explorador delante, la tecla no hace nada y lo dice: «Esta tecla usa %CARPETA%,
y no hay ninguna ventana del Explorador delante». Fallar es deliberado: lanzar con
la variable sin resolver acabaría creando una carpeta llamada `%CARPETA%`.

### De dónde sale la ruta

De `IShellWindows`, buscando la ventana que `focus` recordó antes de que el panel
tomara el foco, y pidiéndole `Folder2::Self_().Path()`.

**No se usa `LocationURL`**, que sería bastante menos COM. Medido en este equipo:
una carpeta llamada `Año de prueba 2026` sale como `A%F1o...`, codificada con la
página de códigos ANSI y no en UTF-8; otra llamada `Prueba λ 📁` sale con esos
caracteres **literales, sin codificar**. Las dos formas en la misma cadena y sin
nada que distinga cuál es cuál. `Path()` devuelve UTF-16 exacto en los dos casos.

### Sus límites

- Solo ventanas del **Explorador**. Un gestor de archivos de otra marca o un cuadro
  de «Abrir/Guardar» no están en esa lista de COM.
- «Este equipo», la papelera y el Panel de control son carpetas para el shell, pero
  su ruta es un identificador que no se puede abrir: se rechazan.
- La sustitución llega a `app`, `path`, `script`, `text` y a los pasos de una macro.
  Las direcciones web quedan fuera a propósito: ahí una ruta tendría que ir
  codificada.
- **`script` ejecuta un comando, no abre una consola.** Una tecla de tipo `script`
  cuyo comando sea `%CARPETA%` intenta ejecutar la carpeta y falla; para abrir una
  consola *dentro* de ella, la acción es `app` con la carpeta de trabajo. MiDeck
  rechaza ese caso antes de arrancar nada y lo dice en la tecla, porque si no el
  error se va con la consola al cerrarse y solo se ve un parpadeo.
- Un `script` de tipo CMD recibe su comando tal cual: una carpeta con `&` en el
  nombre lo partiría en dos. Para esos casos usa PowerShell con la ruta
  entrecomillada en los argumentos.

## Perfiles por aplicación

El panel cambia de teclas según la aplicación que tengas delante, como los
perfiles de un Stream Deck. Un perfil **no es un tipo nuevo de cosa**: es una
superficie normal más una regla que dice cuándo mostrarla. El registro plano ya
admitía varios puntos de entrada; solo faltaba declararlos.

```json
"profiles": [
  { "id": "p-excel", "surface": "s-excel", "exes": ["excel.exe"], "enabled": true }
]
```

Viven en `Deck`, **no en `Settings`**, por dos motivos concretos:
`update_settings` reemplaza los ajustes enteros con lo que mande el frontend, así
que un descuido en el editor borraría todos los perfiles; y un perfil referencia
superficies, que es contenido del deck y no una preferencia de ventana.

La lista está **ordenada**: si dos perfiles cubren el mismo ejecutable, gana el
primero. Por eso es una lista y no un mapa, cuyo orden de recorrido no se puede
fijar.

### Crearlos y quitarlos

En **Ajustes → Perfiles por aplicación**. La aplicación se elige de entre las
**que tienes abiertas**, no del menú Inicio: `apps::listar()` devuelve accesos
directos (`.lnk`) y el gancho devuelve ejecutables (`.exe`), así que lo que se
guardara nunca emparejaría con lo que se detecta. Tomándolo de una ventana
abierta, el nombre es exactamente el que el gancho verá después. Queda un campo
de texto para escribirlo a mano.

Un segundo perfil para la misma aplicación **se rechaza**, porque gana el primero
y el segundo no se activaría nunca.

Quitar un perfil **no borra su superficie**, igual que borrar una tecla de carpeta
no borra la carpeta: nada se borra en cascada. Sus teclas quedan guardadas y se
recuperan volviendo a crear el perfil. La confirmación es de dos pulsaciones y no
un `confirm()` del navegador, que en un webview de Tauri bloquea la ventana
entera.

### Por nombre de ejecutable, no por ruta

Se compara `excel.exe`, no la ruta completa. El mismo programa vive en sitios
distintos según se instale por usuario o por máquina —Office y Chrome son los
casos típicos—, y un perfil con la ruta de un equipo no serviría en el de al
lado. El coste conocido es que `javaw.exe` o `python.exe` no distinguen dos
aplicaciones distintas.

### El ancla

Con un perfil activo aparece un botón de ancla en la barra de título. Fijado, el
panel deja de seguir a la aplicación; al soltarlo se va al perfil que toque sin
esperar a que cambies de ventana. Es **estado de sesión y no se guarda**: un ancla
que sobrevive al reinicio deja a alguien atrapado en un perfil sin saber por qué.
La misma acción está en el menú ☰, para que se descubra.

### Tres cosas que había que arreglar antes

Ninguna rompía la compilación, y la primera era destructiva:

1. **`purge_orphans` habría borrado todos los perfiles.** Una superficie de perfil
   no cuelga de la raíz, así que el chequeo de integridad la daba por huérfana y el
   botón «Borrar carpetas sin usar…» de Ajustes se la llevaba sin preguntar. Se
   arregló sembrando el recorrido también desde los perfiles: los tres sitios que
   consumen ese informe se corrigen de golpe.
2. **La celda 0 de un perfil quedaba muerta.** El backend reservaba la celda de
   «Volver» en toda superficie distinta de la raíz, pero un perfil se muestra como
   nivel superior y no tiene nada a lo que volver. Ahora el criterio es «no es una
   superficie base».
3. **Un deck con perfiles los perdía al abrirlo con una versión anterior.** `Deck` y
   `Settings` ganaron captura de campos desconocidos, la misma red que ya tenía
   `Action::Unknown`. **Protege de la v0.3 en adelante, no hacia atrás.**

### El `emit` que no entregaba nada

El aviso de cambio de aplicación nace en un hilo propio, no en un comando. Un
`emit` desde ahí **devuelve `Ok` y no entrega nada**: el evento se pierde en
silencio. Se ve enseguida porque los comandos síncronos de Tauri ya corren en el
hilo principal, así que el mismo evento emitido desde un comando sí llegaba. La
solución es `run_on_main_thread` **solo para emitir**; buscar el ejecutable y
emparejar el perfil se quedan en el hilo trabajador.

### Gestionar un perfil

Cada fila de *Ajustes → Perfiles por aplicación* se puede trabajar entera:

| | |
|---|---|
| **Nombre** | Se edita en el sitio. Sale del título de la ventana al crearlo, que no siempre da algo presentable; antes había que borrar el perfil y rehacerlo, perdiendo las teclas por una cuestión de texto |
| **Ejecutables** | Fichas con `×`. El mismo programa llega con nombres distintos según cómo esté instalado, y hay familias donde las mismas teclas valen para varios. El último no se puede quitar: un perfil sin ejecutables no se activaría jamás |
| **Editar teclas** | Enseña el perfil en el panel **aunque su aplicación no esté abierta**, y lo fija. Antes, a un perfil solo se llegaba teniendo su programa delante: si lo cerrabas, sus teclas quedaban fuera de alcance |
| **Copiar teclas de…** | Trae las de otro perfil a las celdas libres |
| **Exportar / Importar** | Un archivo con el perfil entero, para pasárselo a alguien |

#### Copiar no puede destruir

Las teclas copiadas van **solo a celdas libres**: las que ya hubiera se quedan
donde están. Por eso no hay confirmación —no hace falta una que nadie lee— y por
eso se dice cuántas no cupieron en vez de perderlas en silencio.

Las carpetas se copian enteras, no se comparten. Si el panel de una carpeta fuera
el mismo, editarla en un perfil cambiaría la del otro, que es lo contrario de lo
que espera quien acaba de pedir una copia. Dos teclas que apunten a la misma
carpeta siguen compartiéndola dentro de la copia, y unas carpetas que se apunten
entre sí no cuelgan el programa: se recuerda lo ya copiado.

#### El archivo de perfil

Lleva **un perfil**, no el deck: al importarlo se añade a lo que ya tengas sin
tocar nada más. Un formato de deck completo serviría de respaldo, pero no para
compartir, que es lo que hacía falta.

```json
{
  "version": 1,
  "nombre": "Excel",
  "exes": ["excel.exe"],
  "panel":    { "name": "Excel", "pages": [ … ] },
  "carpetas": { "s-7": { "name": "Pegado especial", "pages": [ … ] } },
  "imagenes": { "a1b2c3.png": "iVBORw0KGgo…" }
}
```

Las imágenes propias **viajan dentro**, en base64. Hace el archivo más pesado, y
es el precio de que el perfil se vea igual en el equipo que lo recibe sin pedir
nada más. Al importarlas, la biblioteca las nombra por contenido, así que dos
personas con la misma imagen acaban compartiendo archivo y las teclas se
reescriben al nombre nuevo.

Lo que se rechaza al importar: un archivo de un formato más nuevo, y un perfil
para una aplicación que **ya tiene uno**. Gana el primero que empareja, así que
el importado no se activaría nunca; dejarlo entrar sería dar por hecho un trabajo
que no funciona.

#### El ancla dice lo que se ve

Con el ancla puesta, lo detectado y lo mostrado dejan de coincidir. Antes el
ancla hablaba de lo detectado, así que decía «perfil *Word* fijado» con las teclas
de Excel delante. Ahora habla del que está en pantalla, que es lo que permite
además enseñar un perfil cuya aplicación no está abierta.

#### Lo que sigue faltando

La detección es **por ejecutable**, y eso es de Windows, no de MiDeck: `javaw.exe`,
`python.exe` o un navegador con varios perfiles son el mismo programa para el
sistema y no se pueden distinguir entre sí. Afinar por título de ventana sería
aditivo si algún día estorba.

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
