//! La carpeta que el Explorador tiene delante, como variable de cualquier tecla.
//!
//! Una macro puede abrir una consola en la carpeta actual a base de `Ctrl+L`,
//! teclear `cmd` e Intro, pero lo hace a ciegas: MiDeck no sabe donde esta, solo
//! aporrea el teclado y confia en que al otro lado haya una barra de direcciones
//! escuchando. Con la ventana equivocada delante, esas tres letras acaban
//! escritas dentro de un documento.
//!
//! Aqui se pregunta la ruta y se recibe como dato. La tecla pasa a ser una accion
//! corriente --`cmd.exe` con carpeta de trabajo-- y si no hay un Explorador
//! delante **no hace nada** y lo dice. Ademas sirve para mas de una tecla: la
//! misma ruta vale para abrir PowerShell, pasarsela a un script o copiarla.

use crate::model::Action;
use std::path::{Path, PathBuf};

/// El nombre de la variable tal como se escribe en el editor.
pub const VARIABLE: &str = "%CARPETA%";

/// La misma, en minusculas ascii, para buscarla sin distinguir mayusculas.
const MARCA: &str = "%carpeta%";

/// El mensaje cuando la tecla pide la carpeta y no hay ninguna.
///
/// Dice que hacer, no solo que fallo: quien pulsa una tecla que no responde
/// necesita saber si esta rota o si es que le falta contexto.
pub const SIN_CARPETA: &str = "Esta tecla usa %CARPETA%, y no hay ninguna ventana del \
                               Explorador delante. Abre la carpeta que quieras usar y \
                               vuelve a pulsarla.";

// --------------------------------------------------------------- sustitucion

/// Reemplaza `%CARPETA%` en los campos donde tiene sentido.
///
/// `resolver` se llama **como mucho una vez, y solo si alguna parte de la accion
/// menciona la variable**: una tecla corriente no paga ni una llamada a COM.
///
/// Es pura respecto a Windows --el unico contacto con el sistema entra por el
/// parametro-- y por eso se puede probar entera.
pub fn con_variables(
    mut accion: Action,
    resolver: impl FnOnce() -> Option<PathBuf>,
) -> Result<Action, String> {
    let mut la_menciona = false;
    recorrer(&mut accion, &mut |campo| {
        if menciona(campo) {
            la_menciona = true;
        }
    });

    if !la_menciona {
        return Ok(accion);
    }

    // Fallar aqui es deliberado. Lanzar con la variable sin resolver crearia una
    // carpeta llamada "%CARPETA%" o abriria la consola en cualquier sitio, y el
    // usuario tardaria en entender por que.
    let Some(carpeta) = resolver() else {
        return Err(SIN_CARPETA.to_string());
    };

    let valor = carpeta.to_string_lossy().to_string();
    recorrer(&mut accion, &mut |campo| *campo = sustituir(campo, &valor));
    Ok(accion)
}

/// Visita los campos de la accion donde una ruta de carpeta significa algo.
///
/// Un solo sitio decide ese conjunto, y lo recorren tanto la deteccion como la
/// sustitucion: en dos listas separadas acabarian desacompasadas, y una tecla
/// pediria la carpeta sin llegar a usarla.
///
/// Queda fuera lo que no admite una ruta: una combinacion de teclas, una accion
/// del catalogo, una carpeta del propio deck y las direcciones web, que ademas
/// tendrian que ir codificadas.
fn recorrer(accion: &mut Action, f: &mut impl FnMut(&mut String)) {
    match accion {
        Action::App {
            target,
            args,
            workdir,
            ..
        } => {
            f(target);
            f(args);
            f(workdir);
        }
        Action::Path { target } => f(target),
        Action::Script { target, args, .. } => {
            f(target);
            f(args);
        }
        Action::Text { text } => f(text),
        Action::Macro { steps } => {
            for paso in steps {
                recorrer(&mut paso.action, f);
            }
        }
        Action::Url { .. }
        | Action::Urls { .. }
        | Action::Folder { .. }
        | Action::Hotkey { .. }
        | Action::System { .. }
        | Action::Unknown { .. } => {}
    }
}

/// Si el texto nombra la variable, en cualquier combinacion de mayusculas.
fn menciona(texto: &str) -> bool {
    texto.to_ascii_lowercase().contains(MARCA)
}

/// Reemplaza todas las apariciones, sin distinguir mayusculas.
///
/// Se busca sobre una copia en minusculas **ascii**: a diferencia de
/// `to_lowercase`, esa no cambia la longitud en bytes de ningun caracter, asi que
/// las posiciones que encuentra siguen valiendo sobre el texto original. Con la
/// otra, una ruta con ciertos caracteres descolocaria el corte.
fn sustituir(texto: &str, valor: &str) -> String {
    let bajo = texto.to_ascii_lowercase();
    let mut out = String::with_capacity(texto.len());
    let mut desde = 0;

    while let Some(pos) = bajo[desde..].find(MARCA) {
        let inicio = desde + pos;
        out.push_str(&texto[desde..inicio]);
        out.push_str(valor);
        desde = inicio + MARCA.len();
    }
    out.push_str(&texto[desde..]);
    out
}

// -------------------------------------------------------------- la ruta real

/// Si la cadena que da el shell es una ruta de disco y no un sitio virtual.
///
/// "Este equipo", la papelera y el Panel de control tambien son carpetas para el
/// shell, pero su ruta es un identificador entre llaves que no se puede abrir ni
/// pasarle a un programa.
pub fn es_ruta_de_disco(ruta: &str) -> bool {
    !ruta.trim().is_empty() && !ruta.starts_with("::") && !ruta.starts_with("shell:")
}

/// La carpeta de la ventana del Explorador que el usuario tenia delante.
///
/// Se recorren las ventanas del shell buscando la que coincide con la que
/// `focus` recordo antes de que el panel tomara el foco, y se le pide su ruta.
///
/// **No se usa `LocationURL`**, que seria bastante menos COM, porque lo que
/// devuelve no hay forma de interpretarlo: medido en este equipo, una carpeta
/// llamada «Ano de prueba» con enie sale como `A%F1o`, codificada con la pagina
/// de codigos ANSI y no en UTF-8, mientras que otra con una lambda y un emoji en
/// el nombre sale con esos caracteres **literales, sin codificar**. Las dos
/// formas en la misma cadena y sin nada que distinga cual es cual. La ruta que da
/// `Folder2::Self_().Path()` es UTF-16 exacta en los dos casos.
///
/// Devuelve `None` tambien cuando lo de delante no es el Explorador: un gestor de
/// archivos de otra marca o un cuadro de "Abrir/Guardar" no salen en esta lista.
#[cfg(windows)]
pub fn carpeta_en_primer_plano() -> Option<PathBuf> {
    use windows::core::Interface;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        Folder2, IShellFolderViewDual, IShellWindows, IWebBrowser2, ShellWindows,
    };

    let objetivo = crate::focus::ventana_anterior()?;

    unsafe {
        // Puede estar ya inicializado por Tauri en este hilo. Solo se deshace lo
        // que se haya hecho aqui: apagarle el COM a otro es peor que no entrar.
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let hay_que_cerrar = hr.is_ok();

        let resultado = (|| {
            let ventanas: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL).ok()?;
            let total = ventanas.Count().ok()?;

            for i in 0..total {
                let Ok(item) = ventanas.Item(&windows::core::VARIANT::from(i)) else {
                    continue;
                };
                let Ok(navegador) = item.cast::<IWebBrowser2>() else {
                    continue;
                };
                let Ok(hwnd) = navegador.HWND() else {
                    continue;
                };
                if hwnd.0 != objetivo.0 as isize {
                    continue;
                }

                // A partir de aqui ya es la ventana correcta: si algo de la
                // cadena falla es que no esta mostrando una carpeta del disco.
                let vista: IShellFolderViewDual = navegador.Document().ok()?.cast().ok()?;
                let carpeta: Folder2 = vista.Folder().ok()?.cast().ok()?;
                let ruta = carpeta.Self_().ok()?.Path().ok()?.to_string();

                return es_ruta_de_disco(&ruta).then(|| PathBuf::from(ruta));
            }
            None
        })();

        if hay_que_cerrar {
            CoUninitialize();
        }
        resultado
    }
}

#[cfg(not(windows))]
pub fn carpeta_en_primer_plano() -> Option<PathBuf> {
    None
}

/// La carpeta de delante, solo si sigue existiendo.
///
/// Entre que se lee la ruta y se lanza la tecla, la ventana pudo cerrarse o la
/// unidad de red caerse. Comprobarlo convierte un lanzamiento que falla con un
/// error del sistema en el mensaje de `SIN_CARPETA`, que si explica que hacer.
pub fn carpeta_disponible() -> Option<PathBuf> {
    carpeta_en_primer_plano().filter(|r| Path::new(r).is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Action, MacroStep, Shell};

    const CARPETA: &str = r"C:\Users\ana\Mis documentos";

    /// `Action` no deriva `PartialEq` y no merece la pena anadirselo solo para
    /// esto: comparar su JSON da ademas una diferencia legible cuando falla.
    fn json(a: &Action) -> serde_json::Value {
        serde_json::to_value(a).unwrap()
    }

    fn app(target: &str, args: &str, workdir: &str) -> Action {
        Action::App {
            target: target.into(),
            args: args.into(),
            workdir: workdir.into(),
            focus_if_running: false,
        }
    }

    fn hay() -> Option<PathBuf> {
        Some(PathBuf::from(CARPETA))
    }

    // ------------------------------------------------- cuando no hace falta

    #[test]
    fn una_tecla_que_no_menciona_la_variable_no_consulta_nada() {
        // Lo que le cuesta a una tecla corriente que esto exista: nada.
        let mut consultas = 0;
        let salida = con_variables(app("notepad.exe", "", r"C:\temp"), || {
            consultas += 1;
            hay()
        })
        .unwrap();

        assert_eq!(consultas, 0, "no deberia haber preguntado por la carpeta");
        assert_eq!(json(&salida), json(&app("notepad.exe", "", r"C:\temp")));
    }

    #[test]
    fn la_carpeta_se_consulta_una_sola_vez_aunque_aparezca_en_varios_campos() {
        let mut consultas = 0;
        con_variables(app("cmd.exe", "%CARPETA%", "%CARPETA%"), || {
            consultas += 1;
            hay()
        })
        .unwrap();

        assert_eq!(consultas, 1);
    }

    // ------------------------------------------------------- la sustitucion

    #[test]
    fn sustituye_en_los_tres_campos_de_una_app() {
        let salida = con_variables(app("cmd.exe", "/K cd %CARPETA%", "%CARPETA%"), hay).unwrap();

        assert_eq!(
            json(&salida),
            json(&app("cmd.exe", &format!("/K cd {CARPETA}"), CARPETA))
        );
    }

    #[test]
    fn da_igual_como_se_escriba() {
        // Las variables de Windows no distinguen mayusculas y nadie espera que
        // esta si lo haga.
        for escrita in ["%CARPETA%", "%carpeta%", "%Carpeta%", "%CaRpEtA%"] {
            let salida = con_variables(app("cmd.exe", "", escrita), hay).unwrap();
            assert_eq!(
                json(&salida),
                json(&app("cmd.exe", "", CARPETA)),
                "fallo con {escrita}"
            );
        }
    }

    #[test]
    fn sustituye_todas_las_apariciones_del_mismo_campo() {
        let salida =
            con_variables(app("robocopy.exe", r"%CARPETA% %CARPETA%\copia", ""), hay).unwrap();

        assert_eq!(
            json(&salida),
            json(&app(
                "robocopy.exe",
                &format!(r"{CARPETA} {CARPETA}\copia"),
                ""
            ))
        );
    }

    #[test]
    fn el_valor_sustituido_no_se_vuelve_a_mirar() {
        // Una carpeta que se llamara justo "%CARPETA%" no puede desencadenar una
        // segunda ronda de sustitucion.
        let rara = PathBuf::from(r"C:\%CARPETA%");
        let salida = con_variables(app("cmd.exe", "", "%CARPETA%"), || Some(rara.clone())).unwrap();

        assert_eq!(json(&salida), json(&app("cmd.exe", "", r"C:\%CARPETA%")));
    }

    #[test]
    fn una_ruta_con_espacios_entrecomillada_sigue_siendo_un_solo_argumento() {
        // El motivo por el que el editor ensena a escribir "%CARPETA%" con
        // comillas: sin ellas, "Mis documentos" se parte en dos argumentos.
        let salida = con_variables(app("code.cmd", "\"%CARPETA%\"", ""), hay).unwrap();
        let Action::App { args, .. } = &salida else {
            panic!("deberia seguir siendo una app");
        };

        assert_eq!(crate::launcher::split_args(args), vec![CARPETA.to_string()]);
    }

    #[test]
    fn tambien_sirve_en_un_script_en_un_path_y_en_un_texto() {
        let script = con_variables(
            Action::Script {
                shell: Shell::Powershell,
                target: r"C:\s.ps1".into(),
                args: "-Ruta \"%CARPETA%\"".into(),
                hidden: false,
            },
            hay,
        )
        .unwrap();
        let Action::Script { args, .. } = &script else {
            panic!("deberia seguir siendo un script");
        };
        assert_eq!(args, &format!("-Ruta \"{CARPETA}\""));

        let ruta = con_variables(
            Action::Path {
                target: r"%CARPETA%\informes".into(),
            },
            hay,
        )
        .unwrap();
        assert_eq!(
            json(&ruta),
            json(&Action::Path {
                target: format!(r"{CARPETA}\informes")
            })
        );

        // Copiar la ruta: una tecla que la macro no puede imitar de ninguna forma.
        let texto = con_variables(
            Action::Text {
                text: "%CARPETA%".into(),
            },
            hay,
        )
        .unwrap();
        assert_eq!(
            json(&texto),
            json(&Action::Text {
                text: CARPETA.to_string()
            })
        );
    }

    #[test]
    fn llega_hasta_dentro_de_los_pasos_de_una_macro() {
        let secuencia = Action::Macro {
            steps: vec![
                MacroStep {
                    action: Box::new(Action::Hotkey {
                        keys: "Ctrl+C".into(),
                    }),
                    delay_ms: 50,
                },
                MacroStep {
                    action: Box::new(app("cmd.exe", "", "%CARPETA%")),
                    delay_ms: 0,
                },
            ],
        };

        let salida = con_variables(secuencia, hay).unwrap();
        let Action::Macro { steps } = &salida else {
            panic!("deberia seguir siendo una macro");
        };
        assert_eq!(json(&steps[1].action), json(&app("cmd.exe", "", CARPETA)));
    }

    #[test]
    fn una_direccion_web_se_queda_como_estaba() {
        // Queda fuera a proposito: una ruta de Windows dentro de una URL tendria
        // que ir codificada, y nadie escribiria eso esperando que funcione.
        let url = Action::Url {
            target: "https://ejemplo/%CARPETA%".into(),
            browser: "default".into(),
            profile: None,
        };
        let mut consultas = 0;
        let salida = con_variables(url.clone(), || {
            consultas += 1;
            hay()
        })
        .unwrap();

        assert_eq!(json(&salida), json(&url));
        assert_eq!(consultas, 0);
    }

    // ------------------------------------------------- cuando no hay carpeta

    #[test]
    fn sin_carpeta_delante_la_tecla_falla_y_no_lanza_nada() {
        // Lo que importa no es el mensaje, es que devuelve Err: si devolviera la
        // accion con la variable sin resolver, Windows acabaria creando una
        // carpeta llamada "%CARPETA%" y nadie entenderia de donde salio.
        let error = con_variables(app("cmd.exe", "", "%CARPETA%"), || None).unwrap_err();

        assert_eq!(error, SIN_CARPETA);
        assert!(
            error.contains("Explorador"),
            "deberia decir que es lo que falta: {error}"
        );
    }

    // ----------------------------------------------------- la ruta que llega

    #[test]
    fn una_ruta_de_disco_se_acepta_tal_cual() {
        // Llegan en UTF-16 exacto, con acentos y emoji incluidos: no hay nada que
        // descodificar, que es justo el motivo de no usar `LocationURL`.
        for buena in [
            r"C:\Users\ana\Documentos",
            r"D:\",
            r"\\servidor\comun\planos",
            "C:\\Datos\\A\u{f1}o fiscal 2026",
            "C:\\Datos\\Planos \u{3bb} \u{1f4c1}",
        ] {
            assert!(es_ruta_de_disco(buena), "deberia valer {buena}");
        }
    }

    #[test]
    fn una_ventana_que_no_muestra_el_disco_no_da_ruta() {
        // "Este equipo", la papelera y el Panel de control tambien son carpetas
        // para el shell, pero su ruta es un identificador que no se puede abrir
        // ni pasarle a un programa. La tecla tiene que fallar con el mensaje, no
        // inventarse una carpeta.
        for raro in [
            "",
            "   ",
            "::{20D04FE0-3AEA-1069-A2D8-08002B30309D}",
            "::{645FF040-5081-101B-9F08-00AA002F954E}",
            "shell:RecycleBinFolder",
        ] {
            assert!(!es_ruta_de_disco(raro), "deberia rechazar {raro:?}");
        }
    }
}
