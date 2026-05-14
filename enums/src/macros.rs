macro_rules! mk_enum {
    ( $( $camel:ident ),* ) => {
        #[derive(Clone, Debug, PartialEq)]
        pub enum Lang {
            $(
                $camel,
            )*
        }
        impl Lang {
            pub fn into_enum_iter() -> impl Iterator<Item=Lang> {
                use Lang::*;
                [$( $camel, )*].into_iter()
            }
        }
    };
}

macro_rules! mk_get_language {
    ( $( ($camel:ident, $name:ident) ),* ) => {
        pub fn get_language(lang: &Lang) -> Language {
            match lang {
                Lang::Kotlin => tree_sitter_kotlin_ng::LANGUAGE.into(),
                Lang::Lua => tree_sitter_lua::LANGUAGE.into(),
                Lang::Java => tree_sitter_java::LANGUAGE.into(),
                Lang::Go => tree_sitter_go::LANGUAGE.into(),
                Lang::Typescript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
                Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
                Lang::Javascript => tree_sitter_javascript::LANGUAGE.into(),
                Lang::Python => tree_sitter_python::LANGUAGE.into(),
                Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
                Lang::Tcl => tree_sitter_tcl::LANGUAGE.into(),
                Lang::Preproc => tree_sitter_preproc::LANGUAGE.into(),
                Lang::Bash => tree_sitter_bash::LANGUAGE.into(),
                Lang::Csharp => tree_sitter_c_sharp::LANGUAGE.into(),
                Lang::Elixir => tree_sitter_elixir::LANGUAGE.into(),
                Lang::Ccomment => tree_sitter_ccomment::LANGUAGE.into(),
                Lang::Cpp => tree_sitter_mozcpp::LANGUAGE.into(),
                Lang::Mozjs => tree_sitter_mozjs::LANGUAGE.into(),
                Lang::Perl => tree_sitter_perl::LANGUAGE.into(),
                Lang::Php => tree_sitter_php::LANGUAGE_PHP.into(),
                Lang::Ruby => tree_sitter_ruby::LANGUAGE.into(),
            }
        }
    };
}

macro_rules! mk_get_language_name {
    ( $( $camel:ident ),* ) => {
        pub fn get_language_name(lang: &Lang) -> &'static str {
            match lang {
                $(
                    Lang::$camel => stringify!($camel),
                )*
            }
        }
    };
}

macro_rules! mk_langs {
    ( $( ($camel:ident, $name:ident) ),* ) => {
        mk_enum!($( $camel ),*);
        mk_get_language!($( ($camel, $name) ),*);
        mk_get_language_name!($( $camel ),*);
    };
}
