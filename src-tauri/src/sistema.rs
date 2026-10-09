//! Catalogo de acciones de Windows.
//!
//! Casi todo lo que un Stream Deck hace con el sistema se alcanza enviando una
//! combinacion de teclas, asi que este modulo no reimplementa nada: pone nombres
//! amigables encima de `teclas.rs` y reserva la llamada directa a la API para los
//! pocos casos en que es mas fiable que el atajo equivalente.
//!
//! El catalogo vive aqui y no duplicado en JavaScript. El editor lo pide con
//! `list_system_commands`, de modo que anadir una accion es una linea y las dos
//! listas no se pueden separar; esa separacion es exactamente el fallo que ya
//! costo que el nombre de una carpeta se quedara desfasado en el desplegable.

use serde::{Deserialize, Serialize};

/// Accion del catalogo. El nombre de cada variante viaja a `deck.json`, asi que
/// renombrar una rompe los decks existentes: solo se anaden.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemCommand {
    // --- captura
    ScreenshotRegion,
    ScreenshotFull,
    ScreenshotWindow,
    ScreenRecord,
    // --- multimedia
    MediaPlayPause,
    MediaNext,
    MediaPrev,
    VolumeUp,
    VolumeDown,
    VolumeMute,
    MicMute,
    // --- sistema
    Lock,
    ShowDesktop,
    TaskView,
    ClipboardHistory,
    EmojiPicker,
    DesktopPrev,
    DesktopNext,
    FileExplorer,
    TaskManager,
    Settings,
    // --- energia. Todas exigen doble confirmacion.
    Sleep,
    SignOut,
    Restart,
    Shutdown,
    EmptyRecycleBin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Familia {
    Captura,
    Multimedia,
    Sistema,
    /// Acciones que cierran sesion, apagan o borran. Todas piden confirmacion.
    Energia,
}

impl Familia {
    /// Titulo del grupo en el desplegable del editor.
    pub fn etiqueta(self) -> &'static str {
        match self {
            Familia::Captura => "Captura de pantalla",
            Familia::Multimedia => "Multimedia y volumen",
            Familia::Sistema => "Sistema",
            Familia::Energia => "Energia (piden confirmacion)",
        }
    }

    /// En el orden en que aparecen en el desplegable.
    pub fn todas() -> [Familia; 4] {
        [
            Familia::Captura,
            Familia::Multimedia,
            Familia::Sistema,
            Familia::Energia,
        ]
    }
}

/// Como se lleva a cabo cada comando.
///
/// No se serializa: al frontend no le importa el mecanismo, solo el nombre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mecanismo {
    /// Una combinacion de teclas, que es como Windows expone casi todo.
    Teclas(&'static str),
    /// Un destino que abre el shell, como `ms-settings:`.
    Shell(&'static str),
    /// Llamada directa a la API de Windows.
    Api,
    /// Captura propia. No la resuelve `launcher::execute` sino `run_action`,
    /// porque necesita los ajustes del usuario y el portapapeles de la
    /// aplicacion, que esa capa no tiene a mano.
    Capturar(crate::captura::Objetivo),
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ComandoInfo {
    pub id: SystemCommand,
    pub etiqueta: &'static str,
    pub familia: Familia,
    /// Nombre de una entrada del mapa ICONOS del frontend.
    pub icono: &'static str,
    /// Exige doble confirmacion en la tecla antes de ejecutarse.
    pub peligroso: bool,
    /// Limitacion real que el editor muestra junto al desplegable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aviso: Option<&'static str>,
    #[serde(skip)]
    pub mecanismo: Mecanismo,
}

/// Abreviatura para que la tabla de abajo se lea como una tabla.
const fn c(
    id: SystemCommand,
    etiqueta: &'static str,
    familia: Familia,
    icono: &'static str,
    mecanismo: Mecanismo,
) -> ComandoInfo {
    ComandoInfo {
        id,
        etiqueta,
        familia,
        icono,
        peligroso: false,
        aviso: None,
        mecanismo,
    }
}

use Familia::{Captura, Energia, Multimedia, Sistema};
use Mecanismo::{Api, Capturar, Shell, Teclas};
use SystemCommand as S;

/// Como `c`, pero marcando la accion como peligrosa. Lo que distingue a estas no
/// es que fallen, sino que aciertan: una pulsacion sin querer apaga el equipo con
/// el trabajo abierto.
const fn p(
    id: SystemCommand,
    etiqueta: &'static str,
    icono: &'static str,
    aviso: &'static str,
) -> ComandoInfo {
    ComandoInfo {
        peligroso: true,
        aviso: Some(aviso),
        ..c(id, etiqueta, Energia, icono, Api)
    }
}

/// El catalogo completo. Una linea por accion.
const CATALOGO: &[ComandoInfo] = &[
    // ------------------------------------------------------------- captura
    ComandoInfo {
        aviso: Some(
            "Abre la superposicion de recorte de Windows: eliges la region con el \
             raton y la imagen queda en el portapapeles.",
        ),
        ..c(
            S::ScreenshotRegion,
            "Capturar una region",
            Captura,
            "camara",
            Teclas("Win+Shift+S"),
        )
    },
    ComandoInfo {
        aviso: Some(
            "Usa la Barra de juegos de Xbox. No graba el Explorador de archivos ni \
             el escritorio, solo ventanas de aplicacion, y si esta deshabilitada \
             por politica esta tecla no hara nada.",
        ),
        ..c(
            S::ScreenRecord,
            "Grabar la pantalla",
            Captura,
            "video",
            Teclas("Win+Alt+R"),
        )
    },
    ComandoInfo {
        aviso: Some(
            "Guarda un PNG de todos los monitores. La carpeta y si se copia al              portapapeles se eligen en Ajustes.",
        ),
        ..c(
            S::ScreenshotFull,
            "Capturar toda la pantalla",
            Captura,
            "camara-pantalla",
            Capturar(crate::captura::Objetivo::Pantalla),
        )
    },
    ComandoInfo {
        aviso: Some(
            "Captura la ventana que tuvieras delante antes de pulsar el panel, no              el panel. Sale entera aunque estuviera tapada.",
        ),
        ..c(
            S::ScreenshotWindow,
            "Capturar la ventana activa",
            Captura,
            "camara-ventana",
            Capturar(crate::captura::Objetivo::VentanaActiva),
        )
    },
    // ---------------------------------------------------------- multimedia
    c(
        S::MediaPlayPause,
        "Reproducir o pausar",
        Multimedia,
        "play",
        Teclas("MediaPlayPause"),
    ),
    c(
        S::MediaNext,
        "Pista siguiente",
        Multimedia,
        "siguiente",
        Teclas("MediaNext"),
    ),
    c(
        S::MediaPrev,
        "Pista anterior",
        Multimedia,
        "anterior",
        Teclas("MediaPrev"),
    ),
    c(
        S::VolumeUp,
        "Subir el volumen",
        Multimedia,
        "volumen-mas",
        Teclas("VolumeUp"),
    ),
    c(
        S::VolumeDown,
        "Bajar el volumen",
        Multimedia,
        "volumen-menos",
        Teclas("VolumeDown"),
    ),
    c(
        S::VolumeMute,
        "Silenciar",
        Multimedia,
        "silencio",
        Teclas("VolumeMute"),
    ),
    ComandoInfo {
        aviso: Some(
            "Corta y abre el microfono predeterminado. No hay tecla de teclado para              esto: va por la API de audio de Windows, asi que lo ve cualquier              programa que lo este usando.",
        ),
        ..c(
            S::MicMute,
            "Silenciar el microfono",
            Multimedia,
            "silencio",
            Api,
        )
    },
    // ------------------------------------------------------------- sistema
    // Por API y no con Win+L: el atajo se puede deshabilitar por politica, y en
    // un equipo gestionado eso es un riesgo real.
    c(S::Lock, "Bloquear el equipo", Sistema, "candado", Api),
    c(
        S::ShowDesktop,
        "Mostrar el escritorio",
        Sistema,
        "escritorio",
        Teclas("Win+D"),
    ),
    c(
        S::TaskView,
        "Vista de tareas",
        Sistema,
        "ventanas",
        Teclas("Win+Tab"),
    ),
    c(
        S::ClipboardHistory,
        "Historial del portapapeles",
        Sistema,
        "portapapeles",
        Teclas("Win+V"),
    ),
    c(
        S::EmojiPicker,
        "Selector de emoji",
        Sistema,
        "emoji",
        Teclas("Win+Punto"),
    ),
    c(
        S::DesktopPrev,
        "Escritorio virtual anterior",
        Sistema,
        "escritorio-izq",
        Teclas("Ctrl+Win+Izquierda"),
    ),
    c(
        S::DesktopNext,
        "Escritorio virtual siguiente",
        Sistema,
        "escritorio-der",
        Teclas("Ctrl+Win+Derecha"),
    ),
    c(
        S::FileExplorer,
        "Explorador de archivos",
        Sistema,
        "folder-open",
        Teclas("Win+E"),
    ),
    c(
        S::TaskManager,
        "Administrador de tareas",
        Sistema,
        "monitor",
        Teclas("Ctrl+Shift+Esc"),
    ),
    c(
        S::Settings,
        "Configuracion de Windows",
        Sistema,
        "ajustes",
        Shell("ms-settings:"),
    ),
    // ------------------------------------------------------------- energia
    p(
        S::Sleep,
        "Suspender el equipo",
        "luna",
        "Pide confirmacion: hay que pulsar la tecla dos veces.",
    ),
    p(
        S::SignOut,
        "Cerrar sesion",
        "salir",
        "Cierra tus aplicaciones. Guarda antes lo que tengas abierto.",
    ),
    p(
        S::Restart,
        "Reiniciar",
        "reiniciar",
        "Cierra tus aplicaciones. Guarda antes lo que tengas abierto.",
    ),
    p(
        S::Shutdown,
        "Apagar",
        "apagar",
        "Cierra tus aplicaciones. Guarda antes lo que tengas abierto.",
    ),
    p(
        S::EmptyRecycleBin,
        "Vaciar la papelera",
        "papelera",
        "Borra definitivamente lo que haya en la papelera. No se puede deshacer.",
    ),
];

pub fn catalogo() -> &'static [ComandoInfo] {
    CATALOGO
}

/// Ficha de un comando. `None` solo si el catalogo esta incompleto, lo que un
/// test impide; devolverlo en vez de entrar en panico evita que un descuido de
/// programacion se convierta en un cierre de la aplicacion en el equipo de nadie.
pub fn info(cmd: SystemCommand) -> Option<&'static ComandoInfo> {
    CATALOGO.iter().find(|c| c.id == cmd)
}

// ------------------------------------------------- lo que consume el editor

/// El catalogo agrupado por familia, en la forma exacta que necesita un
/// `<optgroup>`. El frontend no escribe ni un titulo de grupo.
#[derive(Serialize)]
pub struct Grupo {
    pub familia: Familia,
    pub etiqueta: &'static str,
    pub comandos: Vec<ComandoInfo>,
}

pub fn agrupado() -> Vec<Grupo> {
    Familia::todas()
        .into_iter()
        .map(|f| Grupo {
            familia: f,
            etiqueta: f.etiqueta(),
            comandos: CATALOGO
                .iter()
                .filter(|c| c.familia == f)
                .copied()
                .collect(),
        })
        .collect()
}

// -------------------------------------------------------------- ejecucion

/// Comandos que se resuelven llamando a la API de Windows.
#[cfg(windows)]
pub fn ejecutar_api(cmd: SystemCommand) -> Result<(), String> {
    use windows::Win32::System::Shutdown::{
        ExitWindowsEx, LockWorkStation, EWX_LOGOFF, EWX_REBOOT, EWX_SHUTDOWN, SHUTDOWN_REASON,
    };

    // Razon registrada en el visor de eventos. Sin ella, el apagado aparece como
    // "otro (sin planificar)" y ensucia el historial del equipo.
    // SHTDN_REASON_FLAG_PLANNED. La parte mayor y la menor son
    // SHTDN_REASON_MAJOR_OTHER y _MINOR_OTHER, que valen cero y no hace falta
    // sumarlas.
    const RAZON: u32 = 0x8000_0000;
    // Solo fuerza el cierre de las aplicaciones colgadas. Sin EWX_FORCE: una
    // tecla no tiene por que tirar por la borda el trabajo sin guardar de nadie.
    const SI_CUELGA: u32 = 0x0000_0010; // EWX_FORCEIFHUNG

    match cmd {
        SystemCommand::Lock => unsafe {
            LockWorkStation().map_err(|e| format!("No se pudo bloquear el equipo: {e}"))
        },

        // Leer y escribir en la misma llamada: entre las dos, el usuario podria
        // haberlo cambiado desde Teams, y la tecla acabaria haciendo lo
        // contrario de lo que ensena.
        SystemCommand::MicMute => {
            crate::audio::alternar_silencio(crate::audio::Flujo::Microfono).map(|_| ())
        }

        SystemCommand::Sleep => suspender(),

        SystemCommand::SignOut => unsafe {
            // Cerrar sesion no necesita privilegio; apagar y reiniciar si.
            ExitWindowsEx(EWX_LOGOFF, SHUTDOWN_REASON(RAZON))
                .map_err(|e| format!("No se pudo cerrar la sesion: {e}"))
        },

        SystemCommand::Restart => unsafe {
            habilitar_privilegio_de_apagado()?;
            ExitWindowsEx(
                windows::Win32::System::Shutdown::EXIT_WINDOWS_FLAGS(EWX_REBOOT.0 | SI_CUELGA),
                SHUTDOWN_REASON(RAZON),
            )
            .map_err(|e| format!("No se pudo reiniciar: {e}"))
        },

        SystemCommand::Shutdown => unsafe {
            habilitar_privilegio_de_apagado()?;
            ExitWindowsEx(
                windows::Win32::System::Shutdown::EXIT_WINDOWS_FLAGS(EWX_SHUTDOWN.0 | SI_CUELGA),
                SHUTDOWN_REASON(RAZON),
            )
            .map_err(|e| format!("No se pudo apagar: {e}"))
        },

        SystemCommand::EmptyRecycleBin => vaciar_papelera(),

        otro => Err(format!("{otro:?} no se ejecuta por API.")),
    }
}

/// Suspende el equipo.
///
/// El primer parametro es "hibernar": en falso, suspension normal. El tercero es
/// "deshabilitar los eventos de reanudacion", que se deja en falso para no
/// cambiarle al usuario como se despierta su equipo.
#[cfg(windows)]
fn suspender() -> Result<(), String> {
    use windows::Win32::System::Power::SetSuspendState;
    let ok = unsafe { SetSuspendState(false, false, false) };
    if ok.as_bool() {
        Ok(())
    } else {
        Err("Windows rechazo suspender el equipo.".to_string())
    }
}

/// Apagar y reiniciar exigen el privilegio SeShutdownPrivilege, que un proceso
/// tiene concedido pero **deshabilitado** de nacimiento. Sin activarlo,
/// `ExitWindowsEx` falla con "no se tienen los privilegios necesarios" y la tecla
/// parece rota sin motivo aparente.
#[cfg(windows)]
fn habilitar_privilegio_de_apagado() -> Result<(), String> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE, LUID};
    use windows::Win32::Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
        .map_err(|e| format!("No se pudo abrir el token del proceso: {e}"))?;

        let mut luid = LUID::default();
        let resultado = LookupPrivilegeValueW(
            windows::core::PCWSTR::null(),
            windows::core::w!("SeShutdownPrivilege"),
            &mut luid,
        )
        .map_err(|e| format!("No se encontro el privilegio de apagado: {e}"))
        .and_then(|_| {
            let privilegios = TOKEN_PRIVILEGES {
                PrivilegeCount: 1,
                Privileges: [LUID_AND_ATTRIBUTES {
                    Luid: luid,
                    Attributes: SE_PRIVILEGE_ENABLED,
                }],
            };
            AdjustTokenPrivileges(token, false, Some(&privilegios), 0, None, None)
                .map_err(|e| format!("No se pudo habilitar el privilegio de apagado: {e}"))
        });

        let _ = CloseHandle(token);
        resultado
    }
}

/// Vacia la papelera de todas las unidades, sin dialogo ni sonido.
///
/// Sin confirmacion de Windows a proposito: la confirmacion ya la hizo el usuario
/// pulsando la tecla dos veces, y encadenar dos preguntas para lo mismo acaba en
/// que nadie lee ninguna.
#[cfg(windows)]
fn vaciar_papelera() -> Result<(), String> {
    use windows::Win32::UI::Shell::{
        SHEmptyRecycleBinW, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND,
    };

    // La papelera ya vacia devuelve S_FALSE, que windows-rs considera Ok: no es
    // un fallo y no hay que distinguirlo.
    unsafe {
        SHEmptyRecycleBinW(
            None,
            windows::core::PCWSTR::null(),
            SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND,
        )
    }
    .map_err(|e| format!("No se pudo vaciar la papelera: {e}"))
}

#[cfg(not(windows))]
pub fn ejecutar_api(_cmd: SystemCommand) -> Result<(), String> {
    Err("Las acciones de Windows solo funcionan en Windows.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Lista exhaustiva de variantes. El `match` de abajo no compila si falta
    /// alguna, y eso obliga a darle entrada tambien en el catalogo.
    fn todas() -> Vec<SystemCommand> {
        let v = vec![
            S::ScreenshotRegion,
            S::ScreenshotFull,
            S::ScreenshotWindow,
            S::ScreenRecord,
            S::MediaPlayPause,
            S::MediaNext,
            S::MediaPrev,
            S::VolumeUp,
            S::VolumeDown,
            S::VolumeMute,
            S::MicMute,
            S::Lock,
            S::ShowDesktop,
            S::TaskView,
            S::ClipboardHistory,
            S::EmojiPicker,
            S::DesktopPrev,
            S::DesktopNext,
            S::FileExplorer,
            S::TaskManager,
            S::Settings,
            S::Sleep,
            S::SignOut,
            S::Restart,
            S::Shutdown,
            S::EmptyRecycleBin,
        ];
        for cmd in &v {
            match cmd {
                S::ScreenshotRegion
                | S::ScreenshotFull
                | S::ScreenshotWindow
                | S::ScreenRecord
                | S::MediaPlayPause
                | S::MediaNext
                | S::MediaPrev
                | S::VolumeUp
                | S::VolumeDown
                | S::VolumeMute
                | S::MicMute
                | S::Lock
                | S::ShowDesktop
                | S::TaskView
                | S::ClipboardHistory
                | S::EmojiPicker
                | S::DesktopPrev
                | S::DesktopNext
                | S::FileExplorer
                | S::TaskManager
                | S::Settings
                | S::Sleep
                | S::SignOut
                | S::Restart
                | S::Shutdown
                | S::EmptyRecycleBin => {}
            }
        }
        v
    }

    #[test]
    fn toda_variante_tiene_ficha_en_el_catalogo() {
        for cmd in todas() {
            assert!(info(cmd).is_some(), "{cmd:?} no esta en el catalogo");
        }
    }

    #[test]
    fn no_hay_ids_duplicados() {
        let unicos: HashSet<_> = CATALOGO.iter().map(|c| c.id).collect();
        assert_eq!(unicos.len(), CATALOGO.len(), "hay un id repetido");
    }

    #[test]
    fn toda_entrada_tiene_etiqueta_e_icono() {
        for ficha in CATALOGO {
            assert!(
                !ficha.etiqueta.trim().is_empty(),
                "{:?} sin etiqueta",
                ficha.id
            );
            assert!(!ficha.icono.trim().is_empty(), "{:?} sin icono", ficha.id);
        }
    }

    /// El que de verdad protege: una errata en una combinacion del catalogo no
    /// se notaria hasta que un usuario pulsara la tecla y no pasara nada.
    #[test]
    fn toda_combinacion_del_catalogo_se_entiende() {
        for ficha in CATALOGO {
            if let Mecanismo::Teclas(k) = ficha.mecanismo {
                assert!(
                    crate::teclas::parsear(k).is_ok(),
                    "{:?} tiene una combinacion que no se entiende: {k}",
                    ficha.id
                );
            }
        }
    }

    /// Marcar peligrosa una accion inofensiva molesta; no marcar una destructiva
    /// apaga el equipo de alguien. La correspondencia con la familia Energia es
    /// exacta en los dos sentidos a proposito.
    #[test]
    fn son_peligrosas_exactamente_las_de_energia() {
        for ficha in CATALOGO {
            assert_eq!(
                ficha.peligroso,
                ficha.familia == Familia::Energia,
                "{:?} no coincide: peligroso={} familia={:?}",
                ficha.id,
                ficha.peligroso,
                ficha.familia
            );
        }
        assert!(CATALOGO.iter().any(|f| f.peligroso), "ninguna peligrosa");
    }

    #[test]
    fn toda_accion_peligrosa_explica_que_hace_antes_de_hacerlo() {
        for ficha in CATALOGO.iter().filter(|f| f.peligroso) {
            assert!(ficha.aviso.is_some(), "{:?} sin aviso", ficha.id);
        }
    }

    #[test]
    fn agrupado_devuelve_todas_las_acciones_y_ningun_grupo_vacio() {
        let grupos = agrupado();
        let total: usize = grupos.iter().map(|g| g.comandos.len()).sum();
        assert_eq!(total, CATALOGO.len());
        for g in &grupos {
            assert!(!g.comandos.is_empty(), "grupo vacio: {}", g.etiqueta);
            assert!(!g.etiqueta.trim().is_empty());
        }
    }

    #[test]
    fn la_grabacion_y_el_recorte_avisan_de_sus_limites() {
        for id in [S::ScreenRecord, S::ScreenshotRegion] {
            assert!(info(id).unwrap().aviso.is_some(), "{id:?} sin aviso");
        }
    }
}
