pub mod c_lang;
pub mod cpp;
pub mod html;
pub mod javascript;
pub mod python;
pub mod rust_lang;
pub mod typescript;

use crate::indexer::extractor::LanguageExtractor;
use crate::types::Language;

pub fn get_extractor(lang: Language) -> Box<dyn LanguageExtractor> {
    match lang {
        Language::TypeScript => Box::new(typescript::TypeScriptExtractor),
        Language::JavaScript => Box::new(javascript::JavaScriptExtractor),
        Language::Python => Box::new(python::PythonExtractor),
        Language::Rust => Box::new(rust_lang::RustExtractor),
        Language::Html => Box::new(html::HtmlExtractor),
        Language::C => Box::new(c_lang::CExtractor),
        Language::Cpp => Box::new(cpp::CppExtractor),
    }
}
