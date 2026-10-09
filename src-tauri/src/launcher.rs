//! Ejecucion de las acciones de una tecla.
//!
//! Partido en dos a proposito:
//!   - build_launch() decide QUE lanzar: expande variables, parte argumentos y
//!     elige el mecanismo. Es determinista y esta cubierta por tests.
//!   - execute() solo lanza. Es tan delgada que no hay donde esconder un bug.
//!
//! Todo se lanza desacoplado: cerrar el widget no debe matar lo que abrio.

use std::path::Path;

use crate::model::{Action, Shell};

/// Banderas de CreateProcess.
///
/// `CREATE_NO_WINDOW` le da al hijo una consola que no se ve, y
/// `CREATE_NEW_CONSOLE` una propia y visible. Las dos son excluyentes entre si y
/// con `DETACHED_PROCESS`, que dejaria al hijo **sin ninguna consola**: eso es lo
/// que hacia que una tecla de PowerShell no hiciera nada: el proceso arrancaba,
/// su host no encontraba consola donde iniciarse y moria antes de ejecutar una
/// sola linea. `cmd.exe` lo aguantaba, y por eso tardo en verse.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

/// Banderas de creacion para un hijo, segun si debe verse su consola.
///
/// Es una sola de las tres, nunca una suma: se devuelve en vez de combinarse
/// para que el compilador no deje escribir `a | b` por descuido.
#[cfg(windows)]
pub fn banderas_de_consola(no_window: bool) -> u32 {
    if no_window {
        CREATE_NO_WINDOW
    } else {
        CREATE_NEW_CONSOLE
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchSpec {
    /// Lanzar un ejecutable directamente.
    Process {
        program: String,
        args: Vec<String>,
        workdir: Option<String>,
        no_window: bool,
        /// Si ya hay una copia corriendo, traerla al frente en vez de abrir otra.
        ///
        /// Vive aqui y no solo en la accion porque un paso de macro tambien tiene
        /// que poder hacerlo: antes la marca estaba en el modelo, el editor la
        /// aceptaba y dentro de una macro se ignoraba en silencio.
        enfocar_si_corre: bool,
    },
    /// Delegar en el shell de Windows: resuelve .lnk, carpetas y URLs, y respeta
    /// la app predeterminada del usuario.
    Shell { target: String },
    /// Abrir el Explorador con un archivo seleccionado.
    Reveal { path: String },
    /// No lanza nada: lo resuelve la navegacion del propio deck.
    Navigate { surface: String },
    /// Enviar una combinacion de teclas.
    Keys { combinacion: String },
    /// Teclear un texto literal.
    Type { texto: String },
    /// Comando del catalogo que se resuelve llamando a la API de Windows.
    System {
        command: crate::sistema::SystemCommand,
    },
    /// Captura de pantalla propia. La resuelve `run_action`, no `execute`:
    /// necesita los ajustes del usuario y el portapapeles de la aplicacion.
    Capture { objetivo: crate::captura::Objetivo },
    /// Varias cosas en orden, con una pausa opcional tras cada una.
    ///
    /// Un solo concepto para dos usos: abrir un grupo de direcciones con el
    /// navegador predeterminado --donde no hay una sola linea de comandos que las
    /// acepte todas-- y las macros, que son lo mismo con pausas. La resuelve
    /// `run_action`, no `execute`.
    Secuencia(Vec<Paso>),
}

/// Un paso de una secuencia: que hacer y cuanto esperar despues.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paso {
    pub spec: LaunchSpec,
    pub pausa_ms: u32,
}

impl Paso {
    /// Paso sin espera, que es lo que necesitan las varias direcciones.
    pub fn seguido(spec: LaunchSpec) -> Self {
        Self { spec, pausa_ms: 0 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchError {
    /// El destino esta vacio en deck.json.
    Empty { kind: &'static str },
    /// La ruta no existe. Se devuelve con la ruta ya expandida, que es la que el
    /// usuario necesita ver para corregirla.
    NotFound { target: String },
    /// La accion esta mal escrita: una combinacion de teclas que no se entiende.
    /// Se detecta al validar, antes de enviar nada.
    Invalid { motivo: String },
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LaunchError::Empty { kind } => {
                write!(f, "La accion de tipo {kind} no tiene destino configurado.")
            }
            LaunchError::Invalid { motivo } => write!(f, "{motivo}"),
            LaunchError::NotFound { target } => {
                if target.contains(['\\', '/']) {
                    write!(f, "No se encontro: {target}")
                } else {
                    // Un nombre suelto que no se resolvio: lo util es decir donde
                    // se busco y como arreglarlo, no solo que no aparecio.
                    write!(
                        f,
                        "No se encontro {target}: no esta en el PATH ni registrado                          en Windows. Pon la ruta completa al ejecutable."
                    )
                }
            }
        }
    }
}

impl std::error::Error for LaunchError {}

// ----------------------------------------------------------------- expansion

/// Expande %VARIABLES% de Windows. Una variable inexistente se deja tal cual, que
/// es mas util que borrarla: el mensaje de error muestra que fallo.
pub fn expand_env(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == '%' {
            if let Some(cierre) = bytes[i + 1..].iter().position(|c| *c == '%') {
                let nombre: String = bytes[i + 1..i + 1 + cierre].iter().collect();
                if !nombre.is_empty() {
                    if let Ok(valor) = std::env::var(&nombre) {
                        out.push_str(&valor);
                        i += cierre + 2;
                        continue;
                    }
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// Parte una linea de argumentos respetando comillas dobles.
/// `--perfil "Profile 1" -x` da tres argumentos, no cuatro.
pub fn split_args(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut actual = String::new();
    let mut en_comillas = false;
    let mut hay_token = false;

    for c in input.chars() {
        match c {
            '"' => {
                en_comillas = !en_comillas;
                hay_token = true;
            }
            c if c.is_whitespace() && !en_comillas => {
                if hay_token {
                    args.push(std::mem::take(&mut actual));
                    hay_token = false;
                }
            }
            c => {
                actual.push(c);
                hay_token = true;
            }
        }
    }
    if hay_token {
        args.push(actual);
    }
    args
}

/// Resuelve un destino a una ruta real en disco.
///
/// Si ya es una ruta existente, la devuelve. Si es un nombre suelto como
/// "notepad.exe", lo busca en PATH, igual que haria el shell. Hace falta tanto
/// para validar antes de lanzar como para extraer el icono del ejecutable.
pub fn resolve_program(target: &str) -> Option<std::path::PathBuf> {
    let p = Path::new(target);
    if p.exists() {
        return Some(p.to_path_buf());
    }
    // Si traia separadores, era una ruta concreta y simplemente no existe.
    if target.contains('\\') || target.contains('/') {
        return None;
    }

    let extensiones: Vec<String> = if p.extension().is_some() {
        vec![String::new()]
    } else {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.COM;.BAT;.CMD".to_string())
            .split(';')
            .map(|e| e.to_string())
            .collect()
    };

    let rutas = std::env::var("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&rutas) {
        for ext in &extensiones {
            let candidato = dir.join(format!("{target}{ext}"));
            if candidato.is_file() {
                return Some(candidato);
            }
        }
    }

    // Ultimo recurso: App Paths del registro. Los navegadores no estan en PATH,
    // pero si registrados ahi.
    for ext in &extensiones {
        if let Some(p) = desde_app_paths(&format!("{target}{ext}")) {
            return Some(p);
        }
    }
    None
}

/// Ruta registrada en App Paths para un nombre de ejecutable.
///
/// Windows resuelve nombres sueltos como "chrome.exe" por esta clave del
/// registro, no por PATH: es lo que hace que funcione Win+R. ShellExecute la
/// consulta sola, pero CreateProcess no, y abrir una URL con un navegador
/// concreto pasa por CreateProcess. Sin esto, "edge" y "chrome" no se
/// encontraban nunca aunque estuvieran instalados.
#[cfg(windows)]
fn desde_app_paths(nombre: &str) -> Option<std::path::PathBuf> {
    const APP_PATHS: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths";
    const APP_PATHS_32: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\App Paths";

    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

    // El usuario manda sobre la maquina; la vista de 32 bits va al final porque
    // es donde acaban las instalaciones antiguas.
    let sitios = [
        (HKEY_CURRENT_USER, APP_PATHS),
        (HKEY_LOCAL_MACHINE, APP_PATHS),
        (HKEY_LOCAL_MACHINE, APP_PATHS_32),
    ];

    for (raiz, base) in sitios {
        if let Some(valor) = leer_registro(raiz, &format!("{base}\\{nombre}")) {
            let ruta = std::path::PathBuf::from(limpiar_ruta(&valor));
            if ruta.is_file() {
                return Some(ruta);
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn desde_app_paths(_nombre: &str) -> Option<std::path::PathBuf> {
    None
}

/// Valor predeterminado (sin nombre) de una clave del registro.
#[cfg(windows)]
fn leer_registro(raiz: windows::Win32::System::Registry::HKEY, clave: &str) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{RegGetValueW, RRF_RT_REG_SZ};

    let clave_w: Vec<u16> = std::ffi::OsStr::new(clave)
        .encode_wide()
        .chain(Some(0))
        .collect();

    let mut bytes: u32 = 0;
    unsafe {
        // Primera llamada sin buffer: solo para saber cuanto ocupa.
        if RegGetValueW(
            raiz,
            PCWSTR(clave_w.as_ptr()),
            PCWSTR::null(),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut bytes),
        )
        .is_err()
            || bytes == 0
        {
            return None;
        }

        let mut buffer = vec![0u16; (bytes as usize / 2) + 1];
        let mut tam = bytes;
        if RegGetValueW(
            raiz,
            PCWSTR(clave_w.as_ptr()),
            PCWSTR::null(),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr() as *mut _),
            Some(&mut tam),
        )
        .is_err()
        {
            return None;
        }

        let largo = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
        Some(String::from_utf16_lossy(&buffer[..largo]))
    }
}

/// El valor de App Paths suele venir entrecomillado.
fn limpiar_ruta(valor: &str) -> String {
    valor.trim().trim_matches('"').trim().to_string()
}

// --------------------------------------------------------------- build_launch

pub fn build_launch(action: &Action) -> Result<LaunchSpec, LaunchError> {
    match action {
        Action::Folder { surface } => Ok(LaunchSpec::Navigate {
            surface: surface.clone(),
        }),

        Action::Hotkey { keys } => {
            if keys.trim().is_empty() {
                return Err(LaunchError::Empty { kind: "hotkey" });
            }
            Ok(LaunchSpec::Keys {
                combinacion: keys.trim().to_string(),
            })
        }

        Action::Text { text } => {
            if text.is_empty() {
                return Err(LaunchError::Empty { kind: "text" });
            }
            Ok(LaunchSpec::Type {
                texto: text.clone(),
            })
        }

        Action::App {
            target,
            args,
            workdir,
            focus_if_running,
        } => {
            let target = expand_env(target);
            if target.trim().is_empty() {
                return Err(LaunchError::Empty { kind: "app" });
            }
            let args_v = split_args(&expand_env(args));

            // Un .lnk no lo resuelve CreateProcess, y un destino que no es un
            // ejecutable (un .docx, un .pdf) tiene que abrirlo su app predeterminada:
            // ambos casos pasan por el shell. Solo un ejecutable va por CreateProcess.
            if !parece_ejecutable(&target) {
                return Ok(LaunchSpec::Shell { target });
            }

            let workdir = expand_env(workdir);
            Ok(LaunchSpec::Process {
                program: target,
                args: args_v,
                workdir: (!workdir.trim().is_empty()).then_some(workdir),
                no_window: true,
                enfocar_si_corre: *focus_if_running,
            })
        }

        Action::Url {
            target,
            browser,
            profile,
        } => {
            let target = expand_env(target);
            if target.trim().is_empty() {
                return Err(LaunchError::Empty { kind: "url" });
            }
            if browser.eq_ignore_ascii_case("default") || browser.trim().is_empty() {
                return Ok(LaunchSpec::Shell { target });
            }

            let mut args = Vec::new();
            if let Some(perfil) = profile {
                if !perfil.trim().is_empty() {
                    args.push(format!("--profile-directory={perfil}"));
                }
            }
            args.push(target);

            Ok(LaunchSpec::Process {
                program: programa_de_navegador(browser),
                args,
                workdir: None,
                no_window: true,
                enfocar_si_corre: false,
            })
        }

        Action::Urls {
            targets,
            browser,
            profile,
            new_window,
        } => {
            let limpias: Vec<String> = targets
                .iter()
                .map(|t| expand_env(t).trim().to_string())
                .filter(|t| !t.is_empty())
                .collect();

            if limpias.is_empty() {
                return Err(LaunchError::Empty { kind: "urls" });
            }
            if limpias.len() > MAX_URLS {
                return Err(LaunchError::Invalid {
                    motivo: format!(
                        "Son {} direcciones y el limite es {MAX_URLS}: abrirlas todas \
                         de golpe deja el equipo inutilizable un buen rato.",
                        limpias.len()
                    ),
                });
            }

            // El navegador predeterminado se invoca por el shell, que solo acepta
            // un destino por llamada: se abren una a una y el propio navegador
            // las agrupa como pestanas.
            if browser.eq_ignore_ascii_case("default") || browser.trim().is_empty() {
                return Ok(LaunchSpec::Secuencia(
                    limpias
                        .into_iter()
                        .map(|target| Paso::seguido(LaunchSpec::Shell { target }))
                        .collect(),
                ));
            }

            let program = programa_de_navegador(browser);
            let mut args = Vec::new();
            if let Some(perfil) = profile {
                if !perfil.trim().is_empty() {
                    args.push(format!("--profile-directory={perfil}"));
                }
            }
            // Solo los navegadores de la familia Chromium entienden --new-window
            // junto a una lista de direcciones. Firefox usa otra sintaxis y pasarle
            // esta haria que no abriera nada, que es peor que ignorar la casilla.
            if *new_window && es_chromium(&program) {
                args.push("--new-window".to_string());
            }
            args.extend(limpias);

            Ok(LaunchSpec::Process {
                program,
                args,
                workdir: None,
                no_window: true,
                enfocar_si_corre: false,
            })
        }

        // Una tecla de una version mas nueva. No se ejecuta, pero tampoco se
        // pierde: sigue guardada tal cual vino.
        Action::Unknown { .. } => Err(LaunchError::Invalid {
            motivo: match action.tipo_original() {
                Some(t) => format!(
                    "Esta tecla es de tipo \"{t}\", que esta version de MiDeck no \
                     conoce. Se guardo tal cual: actualiza para poder usarla."
                ),
                None => "Esta tecla tiene una accion que esta version de MiDeck no \
                         conoce. Se guardo tal cual."
                    .to_string(),
            },
        }),

        Action::Macro { steps } => {
            if steps.is_empty() {
                return Err(LaunchError::Empty { kind: "macro" });
            }
            if steps.len() > MAX_PASOS {
                return Err(LaunchError::Invalid {
                    motivo: format!(
                        "La macro tiene {} pasos y el limite es {MAX_PASOS}.",
                        steps.len()
                    ),
                });
            }
            // Con saturating no hay desbordamiento que haga entrar en panico a
            // una compilacion de desarrollo por un numero disparatado en el JSON.
            let total = steps
                .iter()
                .fold(0u32, |acc, s| acc.saturating_add(s.delay_ms));
            if total > MAX_PAUSA_TOTAL_MS {
                return Err(LaunchError::Invalid {
                    motivo: format!(
                        "Las pausas suman {total} ms y el limite es {MAX_PAUSA_TOTAL_MS}: una \
                         tecla no puede quedarse colgada tanto tiempo."
                    ),
                });
            }

            let mut pasos = Vec::with_capacity(steps.len());
            for (i, paso) in steps.iter().enumerate() {
                let n = i + 1;
                match paso.action.as_ref() {
                    // Sin esto, dos macros que se llamaran entre si serian una
                    // recursion infinita.
                    Action::Macro { .. } => {
                        return Err(LaunchError::Invalid {
                            motivo: format!("El paso {n} es otra macro, y eso no se permite."),
                        })
                    }
                    // Navegar a mitad de una secuencia deja el panel en un sitio
                    // que nadie pidio, y los pasos siguientes irian a otra parte.
                    Action::Folder { .. } => {
                        return Err(LaunchError::Invalid {
                            motivo: format!(
                            "El paso {n} entra a una carpeta del deck, que no cabe en una macro."
                        ),
                        })
                    }
                    _ => {}
                }
                let spec =
                    build_launch(paso.action.as_ref()).map_err(|e| LaunchError::Invalid {
                        motivo: format!("El paso {n} de la macro: {e}"),
                    })?;
                pasos.push(Paso {
                    spec,
                    pausa_ms: paso.delay_ms,
                });
            }
            Ok(LaunchSpec::Secuencia(pasos))
        }

        Action::System { command } => {
            use crate::sistema::Mecanismo;
            let ficha = crate::sistema::info(*command).ok_or_else(|| LaunchError::Invalid {
                motivo: format!("La accion de Windows {command:?} ya no esta disponible."),
            })?;
            Ok(match ficha.mecanismo {
                Mecanismo::Teclas(k) => LaunchSpec::Keys {
                    combinacion: k.to_string(),
                },
                Mecanismo::Shell(t) => LaunchSpec::Shell {
                    target: t.to_string(),
                },
                Mecanismo::Api => LaunchSpec::System { command: *command },
                Mecanismo::Capturar(objetivo) => LaunchSpec::Capture { objetivo },
            })
        }

        Action::Path { target } => {
            let target = expand_env(target);
            if target.trim().is_empty() {
                return Err(LaunchError::Empty { kind: "path" });
            }
            let p = Path::new(&target);
            // Un archivo se revela seleccionado en el Explorador; una carpeta se abre.
            if p.is_file() {
                Ok(LaunchSpec::Reveal { path: target })
            } else {
                Ok(LaunchSpec::Shell { target })
            }
        }

        Action::Script {
            shell,
            target,
            args,
            hidden,
        } => {
            let target = expand_env(target);
            if target.trim().is_empty() {
                return Err(LaunchError::Empty { kind: "script" });
            }

            // Una carpeta no es un comando. El interprete intenta ejecutarla,
            // falla, y como la consola se cierra sola lo unico que se ve es un
            // parpadeo: el error se va con la ventana. Mejor no arrancar y
            // decirlo en la tecla, que es donde se lee.
            //
            // Pasa de verdad con una tecla de script cuyo comando es %CARPETA%,
            // que es la forma natural de equivocarse al querer "abrir una
            // consola aqui". Es la unica vez que esta funcion mira el disco, y
            // vale la pena: la alternativa es un fallo invisible.
            if Path::new(&target).is_dir() {
                return Err(LaunchError::Invalid {
                    motivo: format!(
                        "«{target}» es una carpeta, no un script ni un comando. Para abrir                          una consola dentro de ella, usa una accion Aplicacion con                          powershell.exe y esa carpeta en «Carpeta de trabajo»."
                    ),
                });
            }

            let extra = split_args(&expand_env(args));
            let bajo = target.to_ascii_lowercase();

            let (program, mut argv) = match shell {
                Shell::Powershell => {
                    let mut v = vec![
                        "-NoProfile".to_string(),
                        "-ExecutionPolicy".to_string(),
                        "Bypass".to_string(),
                    ];
                    if *hidden {
                        v.push("-WindowStyle".to_string());
                        v.push("Hidden".to_string());
                    }
                    if bajo.ends_with(".ps1") {
                        v.push("-File".to_string());
                        v.push(target.clone());
                    } else {
                        // No es un archivo: se trata como comando suelto.
                        v.push("-Command".to_string());
                        v.push(target.clone());
                    }
                    ("powershell.exe".to_string(), v)
                }
                Shell::Cmd => {
                    let v = vec!["/c".to_string(), target.clone()];
                    ("cmd.exe".to_string(), v)
                }
            };
            argv.extend(extra);

            Ok(LaunchSpec::Process {
                program,
                args: argv,
                workdir: None,
                no_window: *hidden,
                enfocar_si_corre: false,
            })
        }
    }
}

/// Un nombre suelto como "notepad.exe" lo resuelve el PATH; una ruta con
/// separadores tiene que existir.
fn parece_ejecutable(target: &str) -> bool {
    let bajo = target.to_ascii_lowercase();
    bajo.ends_with(".exe")
        || bajo.ends_with(".com")
        || bajo.ends_with(".bat")
        || bajo.ends_with(".cmd")
}

fn programa_de_navegador(browser: &str) -> String {
    match browser.to_ascii_lowercase().as_str() {
        "edge" | "msedge" => "msedge.exe".to_string(),
        "chrome" => "chrome.exe".to_string(),
        "firefox" => "firefox.exe".to_string(),
        otro => otro.to_string(), // ruta completa a un .exe
    }
}

/// Cuantas direcciones puede abrir una sola tecla.
///
/// No es una limitacion tecnica sino de sentido comun: una tecla con doscientas
/// direcciones no es una tecla util, es un accidente.
pub const MAX_URLS: usize = 20;

/// Cuantos pasos puede tener una macro.
///
/// No es una limitacion tecnica sino de sentido comun, como la de las URLs: una
/// macro de cincuenta pasos no es una tecla, es un script, y para eso ya existe
/// la accion de tipo `script`.
pub const MAX_PASOS: usize = 20;

/// Cuanto pueden sumar las pausas de una macro.
///
/// Mientras corre, la tecla esta ocupada. Un tope evita que una errata en los
/// milisegundos deje el deck pensando medio minuto sin que nadie entienda por que.
pub const MAX_PAUSA_TOTAL_MS: u32 = 10_000;

/// Si el ejecutable es de la familia Chromium, que es la que acepta
/// `--new-window` seguido de una lista de direcciones.
fn es_chromium(programa: &str) -> bool {
    let bajo = programa.to_ascii_lowercase();
    ["chrome", "msedge", "brave", "vivaldi", "opera", "chromium"]
        .iter()
        .any(|n| bajo.contains(n))
}

// -------------------------------------------------------------- verificacion

/// Comprueba que el destino existe antes de lanzar, para poder marcar la tecla y
/// mostrar la ruta exacta en vez de fallar en silencio.
pub fn validate(spec: &LaunchSpec) -> Result<(), LaunchError> {
    match spec {
        LaunchSpec::Navigate { .. } => Ok(()),
        // Una errata en la combinacion se detecta aqui y marca la tecla en rojo,
        // igual que una ruta inexistente: no se envia nada a medias.
        LaunchSpec::Keys { combinacion } => crate::teclas::parsear(combinacion)
            .map(|_| ())
            .map_err(|motivo| LaunchError::Invalid { motivo }),
        LaunchSpec::Type { texto } => {
            if texto.is_empty() {
                Err(LaunchError::Empty { kind: "text" })
            } else {
                Ok(())
            }
        }
        // El catalogo ya garantiza que el comando existe: lo comprobo
        // build_launch al construir esta variante.
        LaunchSpec::System { .. } => Ok(()),
        // Nada que comprobar contra el disco: la carpeta se crea al guardar.
        LaunchSpec::Capture { .. } => Ok(()),
        // Si un solo paso no es valido, la tecla se marca en rojo entera: mas
        // vale no hacer nada que dejar una macro a medias o abrir la mitad de un
        // grupo de direcciones.
        LaunchSpec::Secuencia(pasos) => {
            if pasos.is_empty() {
                return Err(LaunchError::Empty { kind: "urls" });
            }
            // El numero de paso en el mensaje es la diferencia entre "la macro
            // esta mal" y saber que arreglar.
            pasos.iter().enumerate().try_for_each(|(i, p)| {
                validate(&p.spec).map_err(|e| LaunchError::Invalid {
                    motivo: format!("Paso {}: {e}", i + 1),
                })
            })
        }
        LaunchSpec::Reveal { path } => existe(path),
        LaunchSpec::Shell { target } => {
            // Una URL no se comprueba contra el disco.
            if es_url(target) {
                Ok(())
            } else {
                existe(target)
            }
        }
        LaunchSpec::Process { program, .. } => {
            // Se comprueba tambien el PATH, asi un nombre mal escrito se detecta
            // antes de lanzar y no acaba en un fallo opaco de CreateProcess.
            if resolve_program(program).is_some() {
                Ok(())
            } else {
                Err(LaunchError::NotFound {
                    target: program.to_string(),
                })
            }
        }
    }
}

fn existe(target: &str) -> Result<(), LaunchError> {
    if Path::new(target).exists() {
        Ok(())
    } else {
        Err(LaunchError::NotFound {
            target: target.to_string(),
        })
    }
}

/// Si el destino lleva un esquema (`https:`, `mailto:`, `ms-settings:`) y por
/// tanto lo abre un protocolo, no el sistema de archivos.
///
/// La regla es la de un URI: una letra seguida de letras, digitos, `+`, `-` o
/// `.`, y luego dos puntos. Se exigen al menos dos caracteres antes de los dos
/// puntos justo para que una unidad de Windows (`C:\Users`) no cuente como
/// esquema, que es el unico caso ambiguo en la practica.
pub fn es_url(target: &str) -> bool {
    let Some(pos) = target.find(':') else {
        return false;
    };
    if pos < 2 {
        return false;
    }
    let esquema = &target[..pos];
    esquema.starts_with(|c: char| c.is_ascii_alphabetic())
        && esquema
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
}

// ------------------------------------------------------------------ ejecucion

/// Ruta con la que invocar realmente a un programa.
///
/// Si el nombre se puede resolver (PATH o App Paths), se usa la ruta completa.
/// Si no, se deja tal cual para que el error del sistema sea el que se muestre.
pub fn programa_a_lanzar(program: &str) -> String {
    resolve_program(program)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| program.to_string())
}

/// Lanza lo que describe el spec. Capa delgada a proposito.
pub fn execute(spec: &LaunchSpec) -> Result<(), String> {
    validate(spec).map_err(|e| e.to_string())?;

    match spec {
        LaunchSpec::Navigate { .. } => Ok(()),

        // Antes de enviar teclas hay que devolver el foco: al pulsar la tecla lo
        // tiene el panel, y un Ctrl+S dirigido a Excel llegaria aqui. Si no hay
        // ventana recordada se envia igual, porque los atajos de sistema
        // (Win+algo) funcionan tenga el foco quien lo tenga.
        LaunchSpec::Keys { combinacion } => {
            crate::focus::devolver_foco();
            crate::teclas::enviar(combinacion)
        }

        LaunchSpec::Type { texto } => {
            crate::focus::devolver_foco();
            crate::teclas::escribir(texto)
        }

        LaunchSpec::System { command } => crate::sistema::ejecutar_api(*command),

        // Si se llega aqui es que run_action no la intercepto. Se falla en voz
        // alta en vez de no hacer nada, que seria imposible de diagnosticar.
        LaunchSpec::Capture { .. } => {
            Err("La captura tiene que resolverla run_action, no execute.".to_string())
        }

        // Si se llega aqui es que run_action no la intercepto, igual que con la
        // captura. Se falla en voz alta en vez de no hacer nada.
        LaunchSpec::Secuencia(_) => {
            Err("La secuencia tiene que resolverla run_action, no execute.".to_string())
        }

        LaunchSpec::Process {
            program,
            args,
            workdir,
            no_window,
            enfocar_si_corre,
        } => {
            // Antes esto vivia solo en run_action, que mira la accion de la tecla:
            // dentro de una macro la marca se ignoraba en silencio, porque para
            // entonces ya era un lanzamiento y la accion no estaba. Aqui lo ve
            // todo el mundo igual, la tecla suelta y el paso de macro.
            if *enfocar_si_corre {
                if let Some(ruta) = resolve_program(program) {
                    if crate::focus::focus_running(&ruta) {
                        return Ok(());
                    }
                }
            }

            // CreateProcess tampoco consulta App Paths, asi que no basta con
            // haberlo validado: hay que entregarle la ruta ya resuelta o un
            // "chrome.exe" suelto falla con "program not found".
            let ejecutable = programa_a_lanzar(program);
            let mut cmd = std::process::Command::new(&ejecutable);
            cmd.args(args);
            if let Some(dir) = workdir {
                cmd.current_dir(dir);
            }
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(banderas_de_consola(*no_window));
            }
            cmd.spawn()
                .map(|_| ())
                .map_err(|e| format!("No se pudo lanzar {program}: {e}"))
        }

        LaunchSpec::Shell { target } => open_with_shell(target),

        LaunchSpec::Reveal { path } => {
            // explorer.exe exige /select,<ruta> como UN solo argumento.
            std::process::Command::new("explorer.exe")
                .arg(format!("/select,{path}"))
                .spawn()
                .map(|_| ())
                .map_err(|e| format!("No se pudo abrir el Explorador en {path}: {e}"))
        }
    }
}

/// Abre un destino con el programa que le corresponda segun Windows.
///
/// Se llama a ShellExecuteW directamente, que es lo que hace el propio
/// Explorador: resuelve accesos directos, carpetas, la aplicacion predeterminada
/// de cada extension y los esquemas registrados (`ms-settings:`, `mailto:`).
///
/// Se dejo de delegar en el plugin por dos motivos concretos, los dos vistos al
/// probar la tecla de Configuracion de Windows:
///   - su `open_path` comprueba antes que el destino exista en disco, asi que un
///     esquema como `ms-settings:` fallaba con un "no se encuentra el archivo";
///   - su `open_url` lanza un proceso suelto y da la llamada por buena aunque no
///     se abra nada, de modo que la tecla no fallaba, simplemente no hacia nada.
///
/// ShellExecuteW devuelve un valor menor o igual que 32 cuando falla, asi que un
/// destino que no se abre marca la tecla en rojo en vez de pasar inadvertido.
#[cfg(windows)]
fn open_with_shell(target: &str) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let ancho = |s: &str| -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(Some(0))
            .collect()
    };
    let destino = ancho(target);
    let operacion = ancho("open");

    let r = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(operacion.as_ptr()),
            PCWSTR(destino.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };

    let codigo = r.0 as isize;
    if codigo > 32 {
        Ok(())
    } else {
        Err(format!(
            "No se pudo abrir {target}: Windows devolvio el codigo {codigo}."
        ))
    }
}

#[cfg(not(windows))]
fn open_with_shell(target: &str) -> Result<(), String> {
    tauri_plugin_opener::open_path(target, None::<&str>)
        .map_err(|e| format!("No se pudo abrir {target}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MacroStep;
    use crate::model::{Action, Shell};

    #[test]
    fn un_paso_de_macro_conserva_la_marca_de_traer_al_frente() {
        // La marca viajaba en la accion, y run_action solo miraba la accion de la
        // tecla: dentro de una macro se aceptaba y se ignoraba en silencio, asi
        // que pulsar dos veces dejaba dos copias abiertas. Ahora viaja en el
        // lanzamiento, que es lo que ve quien lo ejecuta.
        let accion = Action::Macro {
            steps: vec![MacroStep {
                action: Box::new(Action::App {
                    target: "notepad.exe".into(),
                    args: String::new(),
                    workdir: String::new(),
                    focus_if_running: true,
                }),
                delay_ms: 0,
            }],
        };

        let LaunchSpec::Secuencia(pasos) = build_launch(&accion).unwrap() else {
            panic!("deberia ser una secuencia");
        };
        let LaunchSpec::Process {
            enfocar_si_corre, ..
        } = &pasos[0].spec
        else {
            panic!("el paso deberia ser un proceso");
        };
        assert!(enfocar_si_corre, "el paso perdio la marca");
    }

    #[test]
    fn sin_la_marca_el_paso_no_la_inventa() {
        let accion = Action::App {
            target: "notepad.exe".into(),
            args: String::new(),
            workdir: String::new(),
            focus_if_running: false,
        };
        let LaunchSpec::Process {
            enfocar_si_corre, ..
        } = build_launch(&accion).unwrap()
        else {
            panic!("deberia ser un proceso");
        };
        assert!(!enfocar_si_corre);
    }

    #[test]
    fn un_navegador_o_un_script_nunca_traen_al_frente() {
        // Abrir una direccion siempre abre la direccion: si se "enfocara" el
        // navegador ya abierto, la tecla no haria nada visible.
        for accion in [
            Action::Url {
                target: "https://ejemplo".into(),
                browser: "chrome".into(),
                profile: None,
            },
            Action::Script {
                shell: Shell::Powershell,
                target: "Get-Date".into(),
                args: String::new(),
                hidden: false,
            },
        ] {
            let LaunchSpec::Process {
                enfocar_si_corre, ..
            } = build_launch(&accion).unwrap()
            else {
                panic!("deberia ser un proceso");
            };
            assert!(!enfocar_si_corre, "{} no deberia enfocar", accion.kind());
        }
    }

    #[test]
    fn una_carpeta_no_es_un_comando() {
        // La tecla "abrir una consola aqui" escrita como script con %CARPETA%:
        // el interprete intenta ejecutar la carpeta, falla, y la consola se
        // cierra antes de que nadie lea el error. Mejor no arrancar.
        let carpeta = std::env::temp_dir();
        let accion = Action::Script {
            shell: Shell::Powershell,
            target: carpeta.display().to_string(),
            args: String::new(),
            hidden: false,
        };

        let e = build_launch(&accion).unwrap_err().to_string();
        assert!(
            e.contains("carpeta"),
            "deberia decir que es una carpeta: {e}"
        );
        assert!(
            e.contains("Carpeta de trabajo"),
            "deberia decir como se hace bien: {e}"
        );
    }

    #[test]
    fn un_script_de_verdad_sigue_pasando() {
        // El rechazo solo mira si es un directorio: un comando suelto, que no
        // existe en disco, no puede verse afectado.
        let accion = Action::Script {
            shell: Shell::Powershell,
            target: "Get-Date".to_string(),
            args: String::new(),
            hidden: false,
        };
        assert!(build_launch(&accion).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn una_consola_oculta_y_una_visible_son_banderas_distintas_y_sueltas() {
        // DETACHED_PROCESS (0x8) estuvo aqui sumado a CREATE_NO_WINDOW y dejaba
        // al hijo sin ninguna consola: PowerShell arrancaba, su host no
        // encontraba donde iniciarse y moria antes de ejecutar una linea, con
        // spawn() devolviendo Ok. Una tecla de script no hacia nada y no habia
        // error que mirar. Este test existe para que no vuelva a sumarse.
        let oculta = banderas_de_consola(true);
        let visible = banderas_de_consola(false);

        assert_eq!(
            oculta, 0x0800_0000,
            "oculta deberia ser solo CREATE_NO_WINDOW"
        );
        assert_eq!(
            visible, 0x0000_0010,
            "visible deberia ser solo CREATE_NEW_CONSOLE"
        );
        assert_eq!(oculta & 0x0000_0008, 0, "no puede llevar DETACHED_PROCESS");
        assert_eq!(visible & 0x0000_0008, 0, "no puede llevar DETACHED_PROCESS");
        assert_eq!(oculta & visible, 0, "son excluyentes, no se combinan");
    }

    fn app(target: &str, args: &str) -> Action {
        Action::App {
            target: target.into(),
            args: args.into(),
            workdir: String::new(),
            focus_if_running: false,
        }
    }

    // --------------------------------------------------------------- utilidades

    #[test]
    fn expande_variables_de_entorno() {
        std::env::set_var("MIDECK_PRUEBA", "C:\\Datos");
        assert_eq!(expand_env("%MIDECK_PRUEBA%\\x.txt"), "C:\\Datos\\x.txt");
    }

    #[test]
    fn una_variable_inexistente_se_deja_visible() {
        // Borrarla silenciosamente dejaria una ruta rota e inexplicable.
        assert_eq!(expand_env("%NO_EXISTE_JAMAS%\\x"), "%NO_EXISTE_JAMAS%\\x");
    }

    #[test]
    fn parte_argumentos_respetando_comillas() {
        assert_eq!(
            split_args("--perfil \"Profile 1\" -x"),
            vec!["--perfil", "Profile 1", "-x"]
        );
        assert_eq!(split_args("   "), Vec::<String>::new());
        assert_eq!(
            split_args("\"C:\\Con Espacios\\a.txt\""),
            vec!["C:\\Con Espacios\\a.txt"]
        );
    }

    // ------------------------------------------------------------------- app

    #[test]
    fn un_exe_con_argumentos_se_lanza_como_proceso() {
        let spec = build_launch(&app("C:\\Win\\app.exe", "--flag \"dos palabras\"")).unwrap();
        assert_eq!(
            spec,
            LaunchSpec::Process {
                program: "C:\\Win\\app.exe".into(),
                args: vec!["--flag".into(), "dos palabras".into()],
                workdir: None,
                no_window: true,
                enfocar_si_corre: false,
            }
        );
    }

    #[test]
    fn un_acceso_directo_pasa_siempre_por_el_shell() {
        // CreateProcess no resuelve .lnk; ShellExecute si.
        let spec = build_launch(&app("C:\\Escritorio\\App.lnk", "--con-args")).unwrap();
        assert_eq!(
            spec,
            LaunchSpec::Shell {
                target: "C:\\Escritorio\\App.lnk".into()
            }
        );
    }

    #[test]
    fn un_destino_vacio_da_error_en_vez_de_lanzar_nada() {
        assert_eq!(
            build_launch(&app("   ", "")),
            Err(LaunchError::Empty { kind: "app" })
        );
    }

    // ------------------------------------------------------------------- url

    #[test]
    fn una_url_con_navegador_predeterminado_va_al_shell() {
        let a = Action::Url {
            target: "https://pancanal.com".into(),
            browser: "default".into(),
            profile: None,
        };
        assert_eq!(
            build_launch(&a).unwrap(),
            LaunchSpec::Shell {
                target: "https://pancanal.com".into()
            }
        );
    }

    #[test]
    fn una_url_con_perfil_de_edge_arma_el_argumento_correcto() {
        let a = Action::Url {
            target: "https://pancanal.com".into(),
            browser: "edge".into(),
            profile: Some("Profile 2".into()),
        };
        assert_eq!(
            build_launch(&a).unwrap(),
            LaunchSpec::Process {
                program: "msedge.exe".into(),
                args: vec![
                    "--profile-directory=Profile 2".into(),
                    "https://pancanal.com".into()
                ],
                workdir: None,
                no_window: true,
                enfocar_si_corre: false,
            }
        );
    }

    #[test]
    fn una_url_no_se_valida_contra_el_disco() {
        let spec = LaunchSpec::Shell {
            target: "https://pancanal.com".into(),
        };
        assert!(validate(&spec).is_ok());
    }

    // ------------------------------------------------------------------ path

    #[test]
    fn una_carpeta_se_abre_y_un_archivo_se_revela_seleccionado() {
        let dir = std::env::temp_dir().join("mideck-launcher-path");
        let _ = std::fs::create_dir_all(&dir);
        let archivo = dir.join("hola.txt");
        std::fs::write(&archivo, "x").unwrap();

        let carpeta_spec = build_launch(&Action::Path {
            target: dir.to_string_lossy().to_string(),
        })
        .unwrap();
        assert!(matches!(carpeta_spec, LaunchSpec::Shell { .. }));

        let archivo_spec = build_launch(&Action::Path {
            target: archivo.to_string_lossy().to_string(),
        })
        .unwrap();
        assert!(matches!(archivo_spec, LaunchSpec::Reveal { .. }));
    }

    #[test]
    fn una_ruta_inexistente_falla_con_la_ruta_a_la_vista() {
        let spec = LaunchSpec::Shell {
            target: "C:\\no\\existe\\jamas.txt".into(),
        };
        assert_eq!(
            validate(&spec),
            Err(LaunchError::NotFound {
                target: "C:\\no\\existe\\jamas.txt".into()
            })
        );
    }

    #[test]
    fn un_nombre_suelto_se_busca_en_el_path() {
        let ok = LaunchSpec::Process {
            program: "notepad.exe".into(),
            args: vec![],
            workdir: None,
            no_window: true,
            enfocar_si_corre: false,
        };
        assert!(validate(&ok).is_ok());

        // Un nombre mal escrito se detecta antes de lanzar, en vez de acabar en un
        // fallo opaco de CreateProcess.
        let mal = LaunchSpec::Process {
            program: "notpad.exe".into(),
            args: vec![],
            workdir: None,
            no_window: true,
            enfocar_si_corre: false,
        };
        assert!(matches!(validate(&mal), Err(LaunchError::NotFound { .. })));
    }

    #[test]
    fn limpia_las_comillas_del_valor_del_registro() {
        // App Paths suele guardar la ruta entrecomillada.
        assert_eq!(
            limpiar_ruta(r#""C:\Program Files\Google\Chrome\Application\chrome.exe""#),
            r"C:\Program Files\Google\Chrome\Application\chrome.exe"
        );
        assert_eq!(limpiar_ruta(r"  C:\x\y.exe  "), r"C:\x\y.exe");
    }

    #[cfg(windows)]
    #[test]
    fn al_lanzar_un_navegador_se_usa_su_ruta_completa() {
        // CreateProcess no resuelve nombres sueltos por App Paths: si se le pasa
        // "chrome.exe" falla con "program not found" aunque este instalado.
        for navegador in ["msedge.exe", "chrome.exe"] {
            if desde_app_paths(navegador).is_none() {
                continue; // no instalado en este equipo
            }
            let ruta = programa_a_lanzar(navegador);
            assert_ne!(ruta, navegador, "{navegador} se paso sin resolver");
            assert!(
                std::path::Path::new(&ruta).is_file(),
                "{navegador} resolvio a algo que no existe: {ruta}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn encuentra_los_navegadores_aunque_no_esten_en_el_path() {
        // Edge y Chrome no se anaden al PATH: Windows los resuelve por App Paths
        // del registro. Sin consultarla, abrir una URL con un navegador concreto
        // fallaba siempre con "no se encontro msedge.exe".
        for navegador in ["msedge.exe", "chrome.exe"] {
            let en_path = std::env::var("PATH")
                .unwrap_or_default()
                .split(';')
                .any(|d| std::path::Path::new(d).join(navegador).is_file());
            assert!(
                !en_path,
                "{navegador} esta en PATH: la premisa del test cambio"
            );

            // Solo se exige si el navegador esta instalado en este equipo.
            if desde_app_paths(navegador).is_some() {
                let r = resolve_program(navegador);
                assert!(
                    r.is_some(),
                    "{navegador} esta registrado pero no se resolvio"
                );
                assert!(r.unwrap().is_file());
            }
        }
    }

    #[test]
    fn resolve_program_encuentra_los_ejecutables_del_sistema() {
        let p = resolve_program("notepad.exe").expect("notepad debe estar en PATH");
        assert!(p.is_file());
        // Sin extension tambien, probando las de PATHEXT.
        assert!(resolve_program("notepad").is_some());
        assert!(resolve_program("no_existe_este_programa_jamas").is_none());
        // Una ruta concreta inexistente no se busca en PATH.
        assert!(resolve_program("C:\\no\\existe\\notepad.exe").is_none());
    }

    // ---------------------------------------------------------------- script

    #[test]
    fn un_ps1_oculto_lleva_file_y_windowstyle_hidden() {
        let a = Action::Script {
            shell: Shell::Powershell,
            target: "C:\\s\\respaldo.ps1".into(),
            args: "-Destino D:\\".into(),
            hidden: true,
        };
        let spec = build_launch(&a).unwrap();
        assert_eq!(
            spec,
            LaunchSpec::Process {
                program: "powershell.exe".into(),
                args: vec![
                    "-NoProfile".into(),
                    "-ExecutionPolicy".into(),
                    "Bypass".into(),
                    "-WindowStyle".into(),
                    "Hidden".into(),
                    "-File".into(),
                    "C:\\s\\respaldo.ps1".into(),
                    "-Destino".into(),
                    "D:\\".into(),
                ],
                workdir: None,
                no_window: true,
                enfocar_si_corre: false,
            }
        );
    }

    #[test]
    fn un_comando_suelto_usa_command_y_no_file() {
        let a = Action::Script {
            shell: Shell::Powershell,
            target: "Get-Date".into(),
            args: String::new(),
            hidden: false,
        };
        let LaunchSpec::Process {
            args, no_window, ..
        } = build_launch(&a).unwrap()
        else {
            panic!("deberia ser un proceso");
        };
        assert!(args.contains(&"-Command".to_string()));
        assert!(!args.contains(&"-File".to_string()));
        // Sin hidden hay consola visible: es lo que el usuario quiere para ver salida.
        assert!(!no_window);
        assert!(!args.contains(&"Hidden".to_string()));
    }

    #[test]
    fn cmd_usa_barra_c() {
        let a = Action::Script {
            shell: Shell::Cmd,
            target: "C:\\s\\tarea.bat".into(),
            args: String::new(),
            hidden: true,
        };
        let spec = build_launch(&a).unwrap();
        assert_eq!(
            spec,
            LaunchSpec::Process {
                program: "cmd.exe".into(),
                args: vec!["/c".into(), "C:\\s\\tarea.bat".into()],
                workdir: None,
                no_window: true,
                enfocar_si_corre: false,
            }
        );
    }

    // ------------------------------------------------- destinos con esquema

    #[test]
    fn un_destino_con_esquema_se_reconoce_como_url() {
        for t in [
            "https://ejemplo.test",
            "http://ejemplo.test",
            "mailto:alguien@pancanal.com",
            "ms-settings:",
            "ms-settings:display",
            "ms-screenclip:",
            "teams:",
        ] {
            assert!(es_url(t), "{t} deberia tratarse como url");
        }
    }

    #[test]
    fn una_ruta_de_windows_no_es_una_url() {
        // El caso ambiguo de verdad: una unidad es una letra y dos puntos. Si
        // contara como esquema, abrir una carpeta dejaria de funcionar.
        for t in [
            r"C:\Users\alguien",
            r"D:\datos\informe.xlsx",
            r"\\servidor\compartido",
            "notepad.exe",
            "informe.pdf",
            "",
        ] {
            assert!(!es_url(t), "{t} no deberia tratarse como url");
        }
    }

    #[test]
    fn ms_settings_no_se_comprueba_contra_el_disco() {
        // Es la razon del fallo que marcaba en rojo la tecla de Configuracion:
        // no es una ruta y no tiene por que existir en ningun sitio.
        let spec = LaunchSpec::Shell {
            target: "ms-settings:".into(),
        };
        assert!(validate(&spec).is_ok());
    }

    // -------------------------------------------------------- varias urls

    fn urls(targets: &[&str], browser: &str, nueva: bool) -> Action {
        Action::Urls {
            targets: targets.iter().map(|t| t.to_string()).collect(),
            browser: browser.into(),
            profile: None,
            new_window: nueva,
        }
    }

    #[test]
    fn varias_urls_en_chrome_van_en_una_sola_llamada() {
        // Es la razon de ser de la accion: una sola invocacion abre una ventana
        // con todas las pestanas, en vez de seis ventanas sueltas.
        let spec = build_launch(&urls(&["https://a", "https://b"], "chrome", true)).unwrap();
        assert_eq!(
            spec,
            LaunchSpec::Process {
                program: "chrome.exe".into(),
                args: vec![
                    "--new-window".into(),
                    "https://a".into(),
                    "https://b".into()
                ],
                workdir: None,
                no_window: true,
                enfocar_si_corre: false,
            }
        );
    }

    #[test]
    fn el_perfil_va_antes_que_las_direcciones() {
        let spec = build_launch(&Action::Urls {
            targets: vec!["https://a".into()],
            browser: "edge".into(),
            profile: Some("Profile 1".into()),
            new_window: false,
        })
        .unwrap();
        let LaunchSpec::Process { args, .. } = spec else {
            panic!("deberia lanzarse como proceso");
        };
        assert_eq!(args, vec!["--profile-directory=Profile 1", "https://a"]);
    }

    #[test]
    fn firefox_no_recibe_la_bandera_de_chromium() {
        // Firefox usa otra sintaxis; pasarle --new-window haria que no abriera
        // nada, que es peor que ignorar la casilla.
        let spec = build_launch(&urls(&["https://a"], "firefox", true)).unwrap();
        let LaunchSpec::Process { args, .. } = spec else {
            panic!("deberia lanzarse como proceso");
        };
        assert_eq!(args, vec!["https://a"]);
    }

    #[test]
    fn con_el_navegador_predeterminado_se_abren_una_a_una() {
        // El shell solo acepta un destino por llamada.
        let spec = build_launch(&urls(&["https://a", "https://b"], "default", false)).unwrap();
        assert_eq!(
            spec,
            LaunchSpec::Secuencia(vec![
                Paso::seguido(LaunchSpec::Shell {
                    target: "https://a".into()
                }),
                Paso::seguido(LaunchSpec::Shell {
                    target: "https://b".into()
                }),
            ])
        );
    }

    #[test]
    fn las_lineas_en_blanco_no_cuentan_como_direcciones() {
        // El campo es un area de texto: sobran saltos de linea al final.
        let spec = build_launch(&urls(&["  https://a  ", "   ", ""], "default", false)).unwrap();
        assert_eq!(
            spec,
            LaunchSpec::Secuencia(vec![Paso::seguido(LaunchSpec::Shell {
                target: "https://a".into()
            })])
        );
    }

    #[test]
    fn una_lista_sin_ninguna_direccion_util_da_error() {
        assert_eq!(
            build_launch(&urls(&["  ", ""], "chrome", false)),
            Err(LaunchError::Empty { kind: "urls" })
        );
    }

    #[test]
    fn demasiadas_direcciones_se_rechazan_antes_de_abrir_nada() {
        let muchas: Vec<String> = (0..MAX_URLS + 1).map(|i| format!("https://s{i}")).collect();
        let a = Action::Urls {
            targets: muchas,
            browser: "chrome".into(),
            profile: None,
            new_window: false,
        };
        let Err(LaunchError::Invalid { motivo }) = build_launch(&a) else {
            panic!("deberia rechazarse por exceso");
        };
        assert!(motivo.contains("21"), "mensaje poco util: {motivo}");
    }

    #[test]
    fn un_grupo_de_urls_se_valida_entero() {
        // Una sola parte invalida marca la tecla: mas vale no abrir nada que
        // abrir media lista.
        let spec = LaunchSpec::Secuencia(vec![
            Paso::seguido(LaunchSpec::Shell {
                target: "https://a".into(),
            }),
            Paso::seguido(LaunchSpec::Shell {
                target: r"C:\no\existe\jamas.txt".into(),
            }),
        ]);
        assert!(validate(&spec).is_err());
    }

    // ----------------------------------------------------------------- macros

    fn paso(a: Action, ms: u32) -> crate::model::MacroStep {
        crate::model::MacroStep {
            action: Box::new(a),
            delay_ms: ms,
        }
    }

    fn atajo(k: &str) -> Action {
        Action::Hotkey { keys: k.into() }
    }

    /// El caso de uso que motiva la funcionalidad: abrir una consola en la
    /// carpeta que el Explorador tiene delante.
    #[test]
    fn una_macro_conserva_el_orden_y_las_pausas() {
        let a = Action::Macro {
            steps: vec![
                paso(atajo("Ctrl+L"), 120),
                paso(Action::Text { text: "cmd".into() }, 50),
                paso(atajo("Intro"), 0),
            ],
        };
        let LaunchSpec::Secuencia(pasos) = build_launch(&a).unwrap() else {
            panic!("una macro deberia dar una secuencia");
        };
        assert_eq!(pasos.len(), 3);
        assert_eq!(
            pasos[0].spec,
            LaunchSpec::Keys {
                combinacion: "Ctrl+L".into()
            }
        );
        assert_eq!(pasos[0].pausa_ms, 120);
        assert_eq!(
            pasos[1].spec,
            LaunchSpec::Type {
                texto: "cmd".into()
            }
        );
        assert_eq!(pasos[1].pausa_ms, 50);
        assert_eq!(pasos[2].pausa_ms, 0);
    }

    /// Sin esto, dos macros que se llamaran entre si serian recursion infinita.
    #[test]
    fn una_macro_dentro_de_otra_se_rechaza() {
        let a = Action::Macro {
            steps: vec![
                paso(atajo("Ctrl+C"), 0),
                paso(
                    Action::Macro {
                        steps: vec![paso(atajo("Ctrl+V"), 0)],
                    },
                    0,
                ),
            ],
        };
        let Err(LaunchError::Invalid { motivo }) = build_launch(&a) else {
            panic!("deberia rechazarse la macro anidada");
        };
        assert!(motivo.contains("paso 2"), "no dice que paso: {motivo}");
    }

    #[test]
    fn un_paso_que_navega_a_una_carpeta_se_rechaza() {
        let a = Action::Macro {
            steps: vec![paso(
                Action::Folder {
                    surface: "s-algo".into(),
                },
                0,
            )],
        };
        let Err(LaunchError::Invalid { motivo }) = build_launch(&a) else {
            panic!("deberia rechazarse el paso de carpeta");
        };
        assert!(motivo.contains("paso 1"), "no dice que paso: {motivo}");
    }

    #[test]
    fn una_macro_demasiado_larga_se_rechaza() {
        let a = Action::Macro {
            steps: (0..MAX_PASOS + 1).map(|_| paso(atajo("F5"), 0)).collect(),
        };
        let Err(LaunchError::Invalid { motivo }) = build_launch(&a) else {
            panic!("deberia rechazarse por larga");
        };
        assert!(motivo.contains("21"), "mensaje poco util: {motivo}");
    }

    #[test]
    fn unas_pausas_que_dejarian_la_tecla_colgada_se_rechazan() {
        let a = Action::Macro {
            steps: vec![paso(atajo("F5"), 6000), paso(atajo("F5"), 6000)],
        };
        let Err(LaunchError::Invalid { motivo }) = build_launch(&a) else {
            panic!("deberia rechazarse por lenta");
        };
        assert!(motivo.contains("12000"), "mensaje poco util: {motivo}");
    }

    /// Un numero disparatado en el JSON no puede hacer entrar en panico a una
    /// compilacion de desarrollo por desbordamiento al sumar.
    #[test]
    fn una_pausa_enorme_no_desborda() {
        let a = Action::Macro {
            steps: vec![paso(atajo("F5"), u32::MAX), paso(atajo("F5"), u32::MAX)],
        };
        assert!(matches!(build_launch(&a), Err(LaunchError::Invalid { .. })));
    }

    #[test]
    fn una_macro_vacia_da_error_en_vez_de_una_tecla_muerta() {
        assert_eq!(
            build_launch(&Action::Macro { steps: vec![] }),
            Err(LaunchError::Empty { kind: "macro" })
        );
    }

    /// Un paso mal escrito marca la tecla **antes de ejecutar ninguno**, y el
    /// mensaje dice cual: es la diferencia entre "la macro fallo" y saber que
    /// arreglar.
    ///
    /// La combinacion se comprueba al validar y no al construir, por el reparto
    /// que ya tenia el modulo. Lo importante es que `validate` recorra la
    /// secuencia entera, porque `run_action` la llama antes del primer paso; sin
    /// eso, una errata en el tercero se descubriria con los dos primeros ya
    /// ejecutados.
    #[test]
    fn un_paso_con_errata_se_detecta_sin_ejecutar_nada() {
        let a = Action::Macro {
            steps: vec![paso(atajo("Ctrl+C"), 0), paso(atajo("Ctrl+Inventada"), 0)],
        };
        let spec = build_launch(&a).expect("construir no comprueba las teclas");

        let Err(LaunchError::Invalid { motivo }) = validate(&spec) else {
            panic!("validar deberia detectar la errata");
        };
        assert!(motivo.contains("Paso 2"), "no dice que paso: {motivo}");
        assert!(motivo.contains("Inventada"), "no dice que fallo: {motivo}");
    }

    /// La razon de que el ejecutor viva en run_action: un paso puede ser una
    /// captura, que `execute` no sabe resolver sola.
    #[test]
    fn un_paso_puede_ser_una_captura() {
        let a = Action::Macro {
            steps: vec![paso(
                Action::System {
                    command: crate::sistema::SystemCommand::ScreenshotFull,
                },
                0,
            )],
        };
        let LaunchSpec::Secuencia(pasos) = build_launch(&a).unwrap() else {
            panic!("deberia dar una secuencia");
        };
        assert!(matches!(pasos[0].spec, LaunchSpec::Capture { .. }));
    }

    #[test]
    fn execute_se_queja_en_voz_alta_si_le_llega_una_secuencia() {
        // Si alguien olvida interceptarla en run_action, tiene que notarse.
        let spec = LaunchSpec::Secuencia(vec![Paso::seguido(LaunchSpec::Keys {
            combinacion: "F5".into(),
        })]);
        let Err(motivo) = execute(&spec) else {
            panic!("deberia fallar");
        };
        assert!(motivo.contains("run_action"), "mensaje poco util: {motivo}");
    }

    // ------------------------------------------------------ catalogo windows

    #[test]
    fn un_comando_de_teclas_del_catalogo_se_convierte_en_envio() {
        use crate::sistema::SystemCommand;
        let spec = build_launch(&Action::System {
            command: SystemCommand::ShowDesktop,
        })
        .unwrap();
        assert_eq!(
            spec,
            LaunchSpec::Keys {
                combinacion: "Win+D".into()
            }
        );
    }

    #[test]
    fn bloquear_el_equipo_pasa_por_la_api_y_no_por_un_atajo() {
        // Win+L se puede deshabilitar por politica; LockWorkStation no.
        use crate::sistema::SystemCommand;
        let spec = build_launch(&Action::System {
            command: SystemCommand::Lock,
        })
        .unwrap();
        assert_eq!(
            spec,
            LaunchSpec::System {
                command: SystemCommand::Lock
            }
        );
    }

    #[test]
    fn la_configuracion_de_windows_se_abre_por_el_shell() {
        use crate::sistema::SystemCommand;
        let spec = build_launch(&Action::System {
            command: SystemCommand::Settings,
        })
        .unwrap();
        assert_eq!(
            spec,
            LaunchSpec::Shell {
                target: "ms-settings:".into()
            }
        );
        // Y no se comprueba contra el disco: ms-settings: no es una ruta.
        assert!(validate(&spec).is_ok());
    }

    #[test]
    fn todo_comando_del_catalogo_produce_un_spec_valido() {
        // Barre el catalogo entero: si una entrada nueva trae una combinacion
        // con errata o un destino imposible, salta aqui y no en el equipo de un
        // usuario.
        for ficha in crate::sistema::catalogo() {
            let spec = build_launch(&Action::System { command: ficha.id })
                .unwrap_or_else(|e| panic!("{:?} no se pudo construir: {e}", ficha.id));
            validate(&spec).unwrap_or_else(|e| panic!("{:?} no valida: {e}", ficha.id));
        }
    }

    // ------------------------------------------------------- teclas y texto

    #[test]
    fn un_atajo_se_convierte_en_envio_de_teclas() {
        let a = Action::Hotkey {
            keys: "  Ctrl+Shift+S  ".into(),
        };
        assert_eq!(
            build_launch(&a).unwrap(),
            LaunchSpec::Keys {
                combinacion: "Ctrl+Shift+S".into()
            }
        );
    }

    #[test]
    fn un_atajo_con_errata_se_detecta_antes_de_enviar_nada() {
        // Esto es lo que marca la tecla en rojo en vez de dejar que el envio
        // falle a medias con medio teclado pulsado.
        let spec = build_launch(&Action::Hotkey {
            keys: "Ctrl+Inventada".into(),
        })
        .unwrap();
        let Err(LaunchError::Invalid { motivo }) = validate(&spec) else {
            panic!("una combinacion imposible deberia invalidarse");
        };
        assert!(motivo.contains("Inventada"), "mensaje poco util: {motivo}");
    }

    #[test]
    fn un_atajo_valido_pasa_la_validacion_sin_tocar_el_disco() {
        let spec = build_launch(&Action::Hotkey {
            keys: "Win+Shift+S".into(),
        })
        .unwrap();
        assert!(validate(&spec).is_ok());
    }

    #[test]
    fn un_atajo_vacio_da_error_en_vez_de_una_tecla_muerta() {
        assert_eq!(
            build_launch(&Action::Hotkey { keys: "   ".into() }),
            Err(LaunchError::Empty { kind: "hotkey" })
        );
    }

    #[test]
    fn un_texto_se_convierte_en_tecleo() {
        let a = Action::Text {
            text: "Añadir señal".into(),
        };
        assert_eq!(
            build_launch(&a).unwrap(),
            LaunchSpec::Type {
                texto: "Añadir señal".into()
            }
        );
    }

    #[test]
    fn un_texto_vacio_da_error() {
        assert_eq!(
            build_launch(&Action::Text {
                text: String::new()
            }),
            Err(LaunchError::Empty { kind: "text" })
        );
    }

    #[test]
    fn los_espacios_de_un_texto_se_respetan() {
        // A diferencia del atajo, aqui no se recorta: un texto puede empezar o
        // acabar con espacio a proposito.
        let a = Action::Text {
            text: "  con margen  ".into(),
        };
        let LaunchSpec::Type { texto } = build_launch(&a).unwrap() else {
            panic!("deberia ser un tecleo");
        };
        assert_eq!(texto, "  con margen  ");
    }

    // ---------------------------------------------------------------- folder

    #[test]
    fn una_carpeta_del_deck_no_lanza_nada() {
        let a = Action::Folder {
            surface: "s-proy".into(),
        };
        assert_eq!(
            build_launch(&a).unwrap(),
            LaunchSpec::Navigate {
                surface: "s-proy".into()
            }
        );
    }
}
