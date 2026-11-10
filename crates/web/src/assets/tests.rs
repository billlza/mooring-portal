//! The assets: how they are named, and what the scripts and the pictures
//! may and may not hold.

use std::collections::BTreeSet;

use super::*;

/// What a script of this site has no business with: connections, turning
/// text into code or markup, other windows, anything kept in the browser,
/// and addresses on other hosts, however they are written. The home
/// page's policy already refuses a script any connection; this keeps the
/// source honest about it.
const FORBIDDEN: [&str; 26] = [
    "fetch(",
    "XMLHttpRequest",
    "WebSocket",
    "EventSource",
    "sendBeacon",
    "import(",
    "eval(",
    "new Function",
    "innerHTML",
    "outerHTML",
    "insertAdjacentHTML",
    "document.write",
    "document.cookie",
    "localStorage",
    "sessionStorage",
    "indexedDB",
    "postMessage",
    "Worker",
    "location",
    ".open(",
    "http://",
    "https://",
    "\"//",
    "'//",
    "`//",
    "sourceMappingURL",
];

/// What `source` holds that a script here may not: forbidden words, and
/// another file brought in by anything but an `import` that is one whole
/// statement to a line. `export … from` brings one in as well, so `from`
/// before a quotation mark counts wherever it stands.
fn offences(source: &str) -> Vec<&str> {
    let mut found: Vec<&str> = FORBIDDEN
        .into_iter()
        .filter(|word| source.contains(word))
        .collect();
    found.extend(source.lines().filter(|line| {
        let close: String = line.split_whitespace().collect();
        let brings_in = line.contains("import")
            || ["from\"", "from'", "from`"]
                .iter()
                .any(|from| close.contains(from));
        brings_in && !(line.starts_with("import ") && line.ends_with("\";"))
    }));
    found
}

/// What each `import` in `source` names.
fn imports(source: &str) -> Vec<&str> {
    source
        .lines()
        .filter(|line| line.starts_with("import "))
        .map(|line| {
            let address = line.strip_suffix("\";").expect(line);
            &address[address.rfind('"').expect(line) + 1..]
        })
        .collect()
}

/// Every colour `shader` names, as the image writes it: `#rrggbb`. A
/// colour is `rgb(r, g, b)` with three whole numbers from 0 to 255.
/// Whatever else stands between `rgb(` and `)` is an error, the function's
/// own definition apart, so that no colour goes unread.
fn colours(shader: &str) -> Result<BTreeSet<String>, String> {
    let mut found = BTreeSet::new();
    for (at, _) in shader.match_indices("rgb(") {
        let inside = &shader[at + 4..];
        let inside = &inside[..inside.find(')').ok_or("a colour is not closed")?];
        if inside == "float r, float g, float b" {
            continue;
        }
        let parts: Vec<f64> = inside
            .split(", ")
            .map(|part| {
                part.parse()
                    .map_err(|_| format!("rgb({inside}) names no colour"))
            })
            .collect::<Result<_, _>>()?;
        let [r, g, b] = parts[..] else {
            return Err(format!("rgb({inside}) is not three numbers"));
        };
        let whole = |part: f64| part.fract() == 0.0 && (0.0..=255.0).contains(&part);
        if !(whole(r) && whole(g) && whole(b)) {
            return Err(format!("rgb({inside}) is not whole numbers up to 255"));
        }
        found.insert(format!("#{:02x}{:02x}{:02x}", r as u8, g as u8, b as u8));
    }
    Ok(found)
}

#[test]
fn fnv1a_matches_its_reference_values() {
    assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    assert_eq!(fnv1a(b"foobar"), 0x8594_4171_f739_67e8);
}

#[test]
fn assets_are_named_by_their_content() {
    let assets = Assets::build();
    let paths = assets.paths();
    for path in [
        &paths.css,
        &paths.mark,
        &paths.hero,
        &paths.land,
        &paths.ring,
        &paths.script,
    ]
    .into_iter()
    .chain(&paths.imported)
    {
        let name = path.strip_prefix("/assets/").unwrap();
        assert!(assets.files.contains_key(name), "{path}");
    }
    assert_eq!(
        paths.css,
        format!("/assets/site.{:016x}.css", fnv1a(STYLESHEET.as_bytes()))
    );
    assert!(assets.files.contains_key("site.css"));
    assert!(format!("{assets:?}").starts_with("Assets { paths"));
    // Each picture is served as itself.
    let drawn = hero::draw();
    for (path, picture) in [
        (&paths.hero, &drawn.image),
        (&paths.land, &drawn.land),
        (&paths.ring, &drawn.ring),
    ] {
        let asset = &assets.files[path.strip_prefix("/assets/").unwrap()];
        assert_eq!(asset.body, picture.as_bytes(), "{path}");
        assert_eq!(asset.content_type, SVG);
        assert_eq!(asset.cache, IMMUTABLE);
    }
}

#[test]
fn the_scripts_share_a_directory_named_by_all_of_them() {
    let assets = Assets::build();
    let directory = script_directory(&MODULES);
    assert_eq!(
        assets.paths().script,
        format!("/assets/{directory}/{ENTRY}")
    );
    for (name, source) in MODULES {
        let asset = &assets.files[&format!("{directory}/{name}")];
        assert_eq!(asset.body, source.as_bytes(), "{name}");
        assert_eq!(asset.content_type, SCRIPT);
        assert_eq!(asset.cache, IMMUTABLE);
    }
    // The page may fetch ahead what the entry imports: all but the entry.
    let ahead: Vec<String> = MODULES
        .iter()
        .filter(|(name, _)| *name != ENTRY)
        .map(|(name, _)| format!("/assets/{directory}/{name}"))
        .collect();
    assert_eq!(assets.paths().imported, ahead);
    assert_eq!(ahead.len(), MODULES.len() - 1);
    // A change to any module, or to its name, moves them all.
    for index in 0..MODULES.len() {
        let mut edited = MODULES;
        edited[index].1 = "export {};\n";
        assert_ne!(script_directory(&edited), directory, "{}", MODULES[index].0);
        let mut renamed = MODULES;
        renamed[index].0 = "other.mjs";
        assert_ne!(script_directory(&renamed), directory);
    }
    // Where a name ends and its source begins is part of the name.
    assert_ne!(
        script_directory(&[("ab", "c")]),
        script_directory(&[("a", "bc")])
    );
}

#[test]
fn every_module_in_the_source_directory_is_served_and_no_test_is() {
    // The modules are the files of `assets/js`; their tests, and what the
    // tests stand the modules on, are in its one subdirectory.
    let directory = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/js");
    let mut files = BTreeSet::new();
    let mut directories = Vec::new();
    for entry in std::fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        if entry.file_type().unwrap().is_dir() {
            directories.push(name);
        } else {
            files.insert(name);
        }
    }
    let served: BTreeSet<String> = MODULES.iter().map(|(name, _)| (*name).to_owned()).collect();
    assert_eq!(served, files);
    assert_eq!(directories, ["test"]);
    assert!(served.iter().all(|name| !name.contains("test")));
    assert_eq!(module(ENTRY), MODULES[0].1);
}

#[test]
fn the_scripts_import_only_each_other() {
    let mut imported = Vec::new();
    for (name, source) in MODULES {
        for address in imports(source) {
            let file = address
                .strip_prefix("./")
                .unwrap_or_else(|| panic!("{name} imports {address}"));
            assert!(
                MODULES.iter().any(|(module, _)| *module == file),
                "{name} imports {address}, which is not served"
            );
            imported.push(file);
        }
    }
    // Every module is reached from the entry, and the entry from the page.
    for (name, _) in MODULES {
        assert_eq!(name == ENTRY, !imported.contains(&name), "{name}");
    }
    assert_eq!(
        imports("import { a } from \"./a.mjs\";\nimport \"./b.mjs\";"),
        ["./a.mjs", "./b.mjs"]
    );
}

#[test]
fn the_scripts_reach_for_nothing_beyond_the_page_they_draw_on() {
    for (name, source) in MODULES {
        assert_eq!(offences(source), Vec::<&str>::new(), "{name}");
        assert!(source.lines().count() <= 600, "{name} is too long");
    }
    // The check itself: each word is caught, and so is an import that the
    // check on imports would not see.
    for word in FORBIDDEN {
        let source = format!("const x = 1;\n  {word}y;\n");
        assert_eq!(offences(&source).first(), Some(&word), "{word}");
    }
    for hidden in [
        "const a = await import\n(\"./a.mjs\");",
        "import { a }\n  from \"./a.mjs\";",
        "export { a } from \"./a.mjs\";",
        "export * from './a.mjs';",
        "export {\n  a,\n}from`./a.mjs`;",
        "const picture = \"//elsewhere.example/x.svg\";",
    ] {
        assert!(!offences(hidden).is_empty(), "{hidden}");
    }
    assert!(offences("import { a } from \"./a.mjs\";\nconst x = a;\n").is_empty());
}

#[test]
fn the_shader_paints_in_the_images_colours_and_with_whole_integers() {
    let shader = module("shader.mjs");
    let image = hero::draw().image;
    let named = colours(shader).unwrap();
    assert!(named.len() >= 10, "{named:?}");
    for colour in &named {
        assert!(
            image.contains(&format!("\"{colour}\"")),
            "the shader paints {colour}, which the image does not use"
        );
    }
    assert_eq!(
        colours(
            "mix(rgb(1.0, 3.0, 11.0), rgb(255.0, 196.0, 107.0), t); \
             vec3 rgb(float r, float g, float b) {}"
        ),
        Ok(BTreeSet::from(["#01030b".to_owned(), "#ffc46b".to_owned()]))
    );
    // A colour the check cannot read is a failure, not a colour let pass.
    for unread in [
        "rgb(13.0,36.0,255.0)",
        "rgb(c.r, c.g, c.b)",
        "rgb(1.0, 2.0)",
        "rgb(1.0, 2.0, 3.0, 4.0)",
        "rgb(1.5, 2.0, 3.0)",
        "rgb(1.0, 2.0, 256.0)",
        "rgb(-1.0, 2.0, 3.0)",
        "rgb(1.0, 2.0, 3.0",
        "vec3 rgb(float r) {}",
    ] {
        assert!(colours(unread).is_err(), "{unread}");
    }
    // Its hash needs 32-bit integers, which a fragment shader must ask for,
    // and it takes no power of a number that may be negative.
    assert!(
        shader.contains("precision highp float;\n") && shader.contains("precision highp int;\n")
    );
    assert!(!shader.contains("pow("));
}

#[test]
fn the_shader_repeats_the_images_shapes() {
    // Where the Milky Way lies, how the lantern glows and how large a star
    // is are written twice: in the drawing, and in the shader that paints
    // the same sky in motion. Each pair is the two writings of one thing;
    // change one, and this fails until the other follows.
    let shader = module("shader.mjs");
    let image = hero::draw().image;
    for (drawn, painted) in [
        (
            "<ellipse cx=\"820\" cy=\"300\" rx=\"900\" ry=\"130\" fill=\"url(#milky)\" \
             transform=\"rotate(-16 820 300)\"/>",
            "turned(slid - vec2(820.0, 300.0), radians(16.0)) / vec2(900.0, 130.0)",
        ),
        (
            "r=\"36\" fill=\"url(#lantern)\"/>",
            "float r = d / (36.0 * (1.0 + 0.04 * waver));",
        ),
        (
            "r=\"2.4\" fill=\"#fff1c8\"/>",
            "smoothstep(2.4 - u_unit, 2.4 + u_unit, d)",
        ),
        (
            "<stop offset=\"0\" stop-color=\"#ffd27a\" stop-opacity=\"0.9\"/>\
             <stop offset=\"0.35\" stop-color=\"#ffb347\" stop-opacity=\"0.32\"/>\
             <stop offset=\"1\" stop-color=\"#ffb347\" stop-opacity=\"0\"/>",
            "float alpha = r < 0.35 ? mix(0.9, 0.32, r / 0.35) : mix(0.32, 0.0, (r - 0.35) / 0.65);",
        ),
    ] {
        assert!(image.contains(drawn), "the image no longer draws {drawn}");
        assert!(
            shader.contains(painted),
            "the shader no longer paints {painted}"
        );
    }
    // A star's size and tint are a rule in the drawing's code, not a
    // figure in what it draws.
    let drawing = include_str!("hero.rs");
    for rule in [
        "radius = 0.35 + 1.3 * size * size * size;",
        "tint < 0.12",
        "tint < 0.2",
    ] {
        assert!(drawing.contains(rule), "the drawing: {rule}");
        assert!(shader.contains(rule), "the shader: {rule}");
    }
}

#[test]
fn the_stylesheet_places_the_hero_in_percentages() {
    // The live scene reads where the image is anchored from the page, and
    // reads only percentages.
    let mut placed = 0;
    for line in STYLESHEET
        .lines()
        .filter(|line| line.contains("object-position"))
    {
        let value = line
            .trim()
            .strip_prefix("object-position: ")
            .and_then(|rest| rest.strip_suffix(';'))
            .unwrap_or_else(|| panic!("{line}"));
        let parts: Vec<&str> = value.split(' ').collect();
        assert_eq!(parts.len(), 2, "{line}");
        for part in parts {
            let number = part.strip_suffix('%').unwrap_or_else(|| panic!("{line}"));
            assert!(number.parse::<f64>().is_ok(), "{line}");
        }
        placed += 1;
    }
    assert!(
        placed >= 2,
        "the hero is placed, and placed again on narrow screens"
    );
}
