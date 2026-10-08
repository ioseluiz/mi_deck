//! Importa una imagen a la biblioteca de MiDeck desde la linea de comandos.
//!
//! Apano mientras no exista el editor visual (fase 3), donde esto se hara
//! arrastrando el archivo sobre la tecla. Usa exactamente el mismo codigo que
//! usara la interfaz, asi que tambien sirve para probar la importacion.
//!
//!   cargo run --example importar_imagen -- C:\ruta\logo.png
//!
//! Imprime el nombre que hay que poner en deck.json:
//!   "icon": { "type": "image", "file": "<lo que imprime>" }

use std::path::Path;

fn main() {
    let Some(ruta) = std::env::args().nth(1) else {
        eprintln!("Uso: cargo run --example importar_imagen -- <ruta a la imagen>");
        std::process::exit(2);
    };

    match mideck::images::import(Path::new(&ruta)) {
        Ok(nombre) => {
            println!("{nombre}");
            eprintln!(
                "Importada en {}",
                mideck::images::resolve(&nombre).display()
            );
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
