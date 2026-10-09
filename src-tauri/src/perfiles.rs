//! Perfiles por aplicacion: que teclas se ven segun lo que tengas delante.
//!
//! Un perfil no es un tipo nuevo de cosa: es una **superficie normal** mas una
//! regla que dice cuando mostrarla. El registro plano de superficies ya admitia
//! varios puntos de entrada; lo unico que faltaba era declararlos.
//!
//! La lista esta ordenada a proposito. Dos perfiles pueden cubrir el mismo
//! ejecutable --uno para "chrome.exe" y otro que tambien lo incluya-- y entonces
//! hace falta una regla de desempate que no dependa del orden en que serde
//! decidiera recorrer un mapa: gana el primero.

use serde::{Deserialize, Serialize};

fn verdadero() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    /// Superficie que se muestra cuando este perfil esta activo.
    pub surface: String,
    /// Nombres de ejecutable, sin ruta y en minusculas: "excel.exe".
    ///
    /// Varios por perfil porque una misma aplicacion a veces son dos binarios
    /// --Office tiene excel.exe y excelcnv.exe-- y porque alguien puede querer
    /// el mismo juego de teclas para Word y PowerPoint.
    #[serde(default)]
    pub exes: Vec<String>,
    #[serde(default = "verdadero")]
    pub enabled: bool,
}

/// Perfil que corresponde a un ejecutable, o `None` si ninguno lo cubre.
///
/// Pura y testeable: equivocarse aqui significa que el panel ensena las teclas de
/// otra aplicacion, y eso no se puede depender de probarlo a mano.
pub fn perfil_para<'a>(perfiles: &'a [Profile], exe: &str) -> Option<&'a Profile> {
    if exe.trim().is_empty() {
        return None;
    }
    perfiles.iter().find(|p| {
        p.enabled
            && p.exes
                .iter()
                .any(|candidato| candidato.trim().eq_ignore_ascii_case(exe.trim()))
    })
}

/// Superficies que son la base de un perfil.
///
/// Hace falta en dos sitios que no se pueden olvidar: el chequeo de integridad,
/// para que un perfil no cuente como carpeta huerfana y acabe borrado, y la
/// reserva de la celda de "Volver", que no tiene sentido en una superficie que se
/// muestra como nivel superior.
pub fn superficies_de_perfil(perfiles: &[Profile]) -> Vec<&str> {
    perfiles.iter().map(|p| p.surface.as_str()).collect()
}

/// Si esa superficie se muestra como nivel superior: la raiz o un perfil.
///
/// Una superficie base no reserva la celda 0, porque no hay nada a lo que volver.
pub fn es_base(root: &str, perfiles: &[Profile], surface_id: &str) -> bool {
    surface_id == root || perfiles.iter().any(|p| p.surface == surface_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn perfil(id: &str, surface: &str, exes: &[&str]) -> Profile {
        Profile {
            id: id.into(),
            surface: surface.into(),
            exes: exes.iter().map(|e| e.to_string()).collect(),
            enabled: true,
        }
    }

    #[test]
    fn empareja_el_ejecutable_de_su_lista() {
        let p = vec![perfil("p1", "s-excel", &["excel.exe"])];
        assert_eq!(
            perfil_para(&p, "excel.exe").map(|x| x.id.as_str()),
            Some("p1")
        );
        assert!(perfil_para(&p, "word.exe").is_none());
    }

    /// Windows no distingue mayusculas en nombres de archivo, y el usuario puede
    /// haberlo escrito a mano.
    #[test]
    fn no_distingue_mayusculas_ni_espacios_sobrantes() {
        let p = vec![perfil("p1", "s", &["  EXCEL.EXE  "])];
        assert!(perfil_para(&p, "excel.exe").is_some());
        assert!(perfil_para(&p, " Excel.Exe ").is_some());
    }

    #[test]
    fn un_perfil_cubre_varios_ejecutables() {
        let p = vec![perfil("p1", "s-office", &["word.exe", "powerpnt.exe"])];
        assert!(perfil_para(&p, "word.exe").is_some());
        assert!(perfil_para(&p, "powerpnt.exe").is_some());
    }

    /// La razon de que la lista sea una lista y no un mapa.
    #[test]
    fn cuando_dos_perfiles_coinciden_gana_el_primero() {
        let p = vec![
            perfil("primero", "s-a", &["chrome.exe"]),
            perfil("segundo", "s-b", &["chrome.exe"]),
        ];
        assert_eq!(
            perfil_para(&p, "chrome.exe").map(|x| x.id.as_str()),
            Some("primero")
        );
    }

    #[test]
    fn un_perfil_deshabilitado_se_salta_y_deja_pasar_al_siguiente() {
        let mut p = vec![
            perfil("apagado", "s-a", &["chrome.exe"]),
            perfil("encendido", "s-b", &["chrome.exe"]),
        ];
        p[0].enabled = false;
        assert_eq!(
            perfil_para(&p, "chrome.exe").map(|x| x.id.as_str()),
            Some("encendido")
        );
    }

    #[test]
    fn sin_perfiles_o_sin_ejecutable_no_hay_nada_que_activar() {
        assert!(perfil_para(&[], "excel.exe").is_none());
        let p = vec![perfil("p1", "s", &["excel.exe"])];
        assert!(perfil_para(&p, "").is_none());
        assert!(perfil_para(&p, "   ").is_none());
    }

    /// Un perfil sin ejecutables no empareja con nada, ni siquiera con la cadena
    /// vacia: si no, cualquier aplicacion desconocida lo activaria.
    #[test]
    fn un_perfil_sin_ejecutables_no_atrapa_todo() {
        let p = vec![perfil("vacio", "s", &[])];
        assert!(perfil_para(&p, "excel.exe").is_none());
        assert!(perfil_para(&p, "").is_none());
    }

    // ------------------------------------------------- superficies base

    #[test]
    fn la_raiz_y_las_superficies_de_perfil_son_base() {
        let p = vec![perfil("p1", "s-excel", &["excel.exe"])];
        assert!(es_base("s-root", &p, "s-root"));
        assert!(es_base("s-root", &p, "s-excel"));
        // Una carpeta corriente no: ahi la celda 0 es la tecla de volver.
        assert!(!es_base("s-root", &p, "s-carpeta"));
    }

    #[test]
    fn un_perfil_deshabilitado_sigue_siendo_base() {
        // Deshabilitarlo no lo convierte en una carpeta ni lo deja huerfano: sus
        // teclas tienen que seguir ahi cuando se vuelva a habilitar.
        let mut p = vec![perfil("p1", "s-excel", &["excel.exe"])];
        p[0].enabled = false;
        assert!(es_base("s-root", &p, "s-excel"));
        assert_eq!(superficies_de_perfil(&p), vec!["s-excel"]);
    }
}
