extern crate base64;
extern crate md5;
#[macro_use]
extern crate nom;

mod spacelike;
mod expression;
mod templateexpression;
mod template;

use nom::IResult::*;

use std::collections::BTreeSet;
use std::fs::{File, create_dir_all, read_dir};
use std::io::{self, Read, Write};
use std::path::Path;
use template::template;

pub fn compile_static_css(indir: &Path, outdir: &Path) -> io::Result<()> {
    let outdir = outdir.join("templates");
    try!(create_dir_all(&outdir));
    File::create(outdir.join("statics.rs")).and_then(|mut f| {
        try!(write!(f,
                    "pub struct StaticFile {{\n  \
                     pub content: &'static [u8],\n  \
                     pub name: &'static str,\n\
                     }}\n"));
        let mut statics = BTreeSet::new();
        for entry in try!(read_dir(indir)) {
            let entry = try!(entry);
            let path = entry.path();
            if let Some(filename) = entry.file_name().to_str() {
                let suffix = ".css";
                if filename.ends_with(suffix) {
                    println!("cargo:rerun-if-changed={}",
                             path.to_string_lossy());
                    let name = &filename[..filename.len() - suffix.len()];
                    let mut input = try!(File::open(&path));
                    let mut buf = Vec::new();
                    try!(input.read_to_end(&mut buf));
                    // TODO Minifying the css would be nice
                    try!(write_static_file(&mut f, &path, name, &buf, suffix));
                    statics.insert(format!("{}_css", name));
                }
                let suffix = ".js";
                if filename.ends_with(suffix) {
                    println!("cargo:rerun-if-changed={}",
                             path.to_string_lossy());
                    let name = &filename[..filename.len() - suffix.len()];
                    let mut input = try!(File::open(&path));
                    let mut buf = Vec::new();
                    try!(input.read_to_end(&mut buf));
                    // TODO Minifying the javascript would be nice
                    try!(write_static_file(&mut f, &path, name, &buf, suffix));
                    statics.insert(format!("{}_js", name));
                }
            }
        }
        try!(write!(f,
                    "\npub static STATICS: &'static [&'static StaticFile] \
                     = &[{}];\n",
                    statics.iter()
                        .map(|s| format!("&{}", s))
                        .collect::<Vec<_>>()
                        .join(", ")));
        Ok(())
    })
}

fn write_static_file(f: &mut Write,
                     path: &Path,
                     name: &str,
                     content: &[u8],
                     suffix: &str)
                     -> io::Result<()> {
    write!(f,
           "\n// From {path:?}\n\
            #[allow(non_upper_case_globals)]\n\
            pub static {name}_{suf}: StaticFile = \
            StaticFile {{\n  \
            content: &{content:?},\n  \
            name: \"{name}-{hash}.{suf}\",\n\
            }};\n",
           path = path,
           name = name,
           content = content,
           hash = checksum_slug(&content),
           suf = &suffix[1..])
}

/// A short and url-safe checksum string from string data.
fn checksum_slug(data: &[u8]) -> String {
    base64::encode_mode(&md5::compute(data)[..6], base64::Base64Mode::UrlSafe)
}


pub fn compile_templates(indir: &Path, outdir: &Path) -> io::Result<()> {
    File::create(outdir.join("templates.rs")).and_then(|mut f| {
        try!(write!(f,
                    "mod templates {{\n\
                     use std::io::{{self, Write}};\n\
                     use std::fmt::Display;\n\n"));

        let outdir = outdir.join("templates");
        try!(create_dir_all(&outdir));

        try!(handle_entries(&mut f, indir, &outdir));

        if outdir.join("statics.rs").exists() {
            try!(write!(f, "pub mod statics;\n"));
        }

        write!(f,
               "{}\n}}\n",
               include_str!(concat!(env!("CARGO_MANIFEST_DIR"),
                                    "/src/template_utils.rs")))
    })
}

fn handle_entries(f: &mut Write,
                  indir: &Path,
                  outdir: &Path)
                  -> io::Result<()> {
    let suffix = ".rs.html";
    for entry in try!(read_dir(indir)) {
        let entry = try!(entry);
        let path = entry.path();
        if try!(entry.file_type()).is_dir() {
            if let Some(filename) = entry.file_name().to_str() {
                let outdir = outdir.join(filename);
                try!(create_dir_all(&outdir));
                try!(File::create(outdir.join("mod.rs"))
                    .and_then(|mut f| handle_entries(&mut f, &path, &outdir)));
                try!(write!(f, "pub mod {name};\n\n", name = filename));
            }

        } else if let Some(filename) = entry.file_name().to_str() {
            if filename.ends_with(suffix) {
                println!("cargo:rerun-if-changed={}", path.to_string_lossy());
                let name = &filename[..filename.len() - suffix.len()];
                if try!(handle_template(name, &path, &outdir)) {
                    try!(write!(f,
                                "mod template_{name};\npub use \
                                 self::template_{name}\
                                 ::{name};\n\n",
                                name = name));
                }
            }
        }
    }
    Ok(())
}

fn handle_template(name: &str, path: &Path, outdir: &Path) -> io::Result<bool> {
    let mut input = try!(File::open(path));
    let mut buf = Vec::new();
    try!(input.read_to_end(&mut buf));
    match template(&buf) {
        Done(_, t) => {
            let fname = outdir.join(format!("template_{}.rs", name));
            try!(File::create(fname)
                .and_then(|mut f| t.write_rust(&mut f, name)));
            Ok(true)
        }
        Error(err) => {
            println!("cargo:warning=\
                      Template parse error in {:?}: {}",
                     path,
                     err);
            Ok(false)
        }
        Incomplete(needed) => {
            println!("cargo:warning=\
                      Failed to parse template {:?}: \
                      {:?} needed",
                     path,
                     needed);
            Ok(false)
        }
    }
}

#[cfg(test)]
mod template_utils_test {
    use std::fmt::Display;
    use std::io::{self, Write};
    include!("template_utils.rs");

    #[test]
    fn encoded() {
        let mut buf = Vec::new();
        "a < b".to_html(&mut buf).unwrap();
        assert_eq!(b"a &lt; b", &buf[..]);
    }
    #[test]
    fn raw_html() {
        let mut buf = Vec::new();
        Html("a<b>c</b>").to_html(&mut buf).unwrap();
        assert_eq!(b"a<b>c</b>", &buf[..]);
    }
}
