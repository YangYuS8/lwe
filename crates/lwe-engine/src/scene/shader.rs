//! Runtime adaptation of user-owned Wallpaper Engine shader assets.
//! No engine shader source is embedded in LWE.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail, ensure};
use lwe_scene::SceneAssets;
use serde_json::Value;

const MAX_SHADER_BYTES: usize = 1024 * 1024;
const MAX_INCLUDE_DEPTH: usize = 24;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Uniform {
    pub name: String,
    pub kind: String,
    pub material: Option<String>,
    pub default: Option<Value>,
    pub texture_default: Option<String>,
    pub texture_combo: Option<String>,
    pub array: bool,
    pub array_length: Option<usize>,
    pub material_range: Option<[f32; 2]>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Shader {
    pub vertex: String,
    pub fragment: String,
    pub uniforms: Vec<Uniform>,
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .enumerate()
            .all(|(i, c)| c.is_ascii_alphabetic() || c == b'_' || (i != 0 && c.is_ascii_digit()))
}

fn metadata(line: &str) -> Result<Option<Value>> {
    let Some(comment) = line.split_once("//").map(|(_, comment)| comment) else {
        return Ok(None);
    };
    let Some(start) = comment.find('{') else {
        return Ok(None);
    };
    Ok(Some(
        serde_json::from_str(&comment[start..]).context("Invalid shader metadata")?,
    ))
}

fn expand(
    assets: &SceneAssets,
    path: &str,
    stack: &mut Vec<String>,
    included: &mut BTreeSet<String>,
    output: &mut String,
) -> Result<()> {
    ensure!(
        stack.len() < MAX_INCLUDE_DEPTH && !stack.iter().any(|item| item == path),
        "unsupported_scene: shader include depth or cycle at {path}"
    );
    if included.contains(path) {
        return Ok(());
    }
    let source = String::from_utf8(assets.read(path, MAX_SHADER_BYTES)?)
        .with_context(|| format!("Shader {path} is not UTF-8"))?;
    expand_source(assets, path, &source, stack, included, output)
}

fn expand_source(
    assets: &SceneAssets,
    path: &str,
    source: &str,
    stack: &mut Vec<String>,
    included: &mut BTreeSet<String>,
    output: &mut String,
) -> Result<()> {
    ensure!(
        stack.len() < MAX_INCLUDE_DEPTH,
        "unsupported_scene: shader include nesting exceeds limit"
    );
    ensure!(
        !stack.iter().any(|item| item == path),
        "unsupported_scene: cyclic shader include {path}"
    );
    if !included.insert(path.to_owned()) {
        return Ok(());
    }
    stack.push(path.to_owned());
    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(include) = trimmed.strip_prefix("#include") {
            let include = include.trim();
            ensure!(
                include.starts_with('"') && include.ends_with('"'),
                "unsupported_scene: unsupported include syntax in {path}"
            );
            let name = &include[1..include.len() - 1];
            let directory = path
                .rsplit_once('/')
                .map(|(dir, _)| dir)
                .unwrap_or("shaders");
            let relative = format!("{directory}/{name}");
            let global = format!("shaders/{name}");
            let resolved = if assets.exists(&relative) {
                relative
            } else {
                global
            };
            expand(assets, &resolved, stack, included, output)?;
        } else if trimmed.starts_with("#require") {
            bail!("unsupported_scene: shader engine extension in {path}: {trimmed}");
        } else if !trimmed.starts_with("#version") {
            output.push_str(line);
            output.push('\n');
        }
        ensure!(
            output.len() <= MAX_SHADER_BYTES,
            "unsupported_scene: expanded shader exceeds limit"
        );
    }
    stack.pop();
    Ok(())
}

fn validate_particle_combos(values: &BTreeMap<String, i32>) -> Result<()> {
    for (name, value) in values {
        let allowed = match name.as_str() {
            // FOG alone is editor metadata; the distance/height branches below
            // supply the actual fog behavior and remain disabled.
            "CUTOUT" | "FOG" | "SPRITESHEET" | "SPRITESHEETBLEND" => matches!(*value, 0 | 1),
            "TEX0FORMAT" => matches!(*value, 0 | 9),
            "LIGHTING"
            | "DOUBLESIDEDLIGHTING"
            | "REFRACT"
            | "NORMALMAP"
            | "FOG_DIST"
            | "FOG_HEIGHT"
            | "ALPHATOCOVERAGE"
            | "LIGHTS_SHADOW_MAPPING"
            | "LIGHTS_COOKIE"
            | "AUDIOPROCESSING"
            | "COPYBG"
            | "TEX1FORMAT" => *value == 0,
            _ => false,
        };
        ensure!(
            allowed,
            "unsupported_scene: particle shader combo {name}={value}"
        );
    }
    Ok(())
}

/// Particle positions, rope geometry, and vertex colors are produced by LWE's
/// bounded CPU simulation. The original fragment still defines the visual
/// material behavior, including overbright and optional alpha cutout.
pub(crate) fn load_particle(
    assets: &SceneAssets,
    overrides: &BTreeMap<String, i32>,
    bound_slots: &BTreeSet<usize>,
) -> Result<Shader> {
    validate_particle_combos(overrides)?;
    ensure!(
        bound_slots.iter().all(|slot| *slot == 0),
        "unsupported_scene: additional particle shader texture slots"
    );
    let path = "shaders/genericparticle.frag";
    let source = String::from_utf8(assets.read(path, MAX_SHADER_BYTES)?)
        .context("Particle shader is not UTF-8")?;
    let mut combos = BTreeMap::new();
    inspect(&source, &mut BTreeMap::new(), &mut combos)?;
    combos.extend(overrides.clone());
    // R8 masks retain their red payload in the RGBA upload; the fragment must
    // retain that source semantic. Other supported formats already supply RGBA.
    combos.entry("TEX0FORMAT".to_owned()).or_insert(0);
    validate_particle_combos(&combos)?;
    let mut platform = combos.clone();
    platform.insert("GLSL".to_owned(), 1);
    // Do this before include expansion: the native fragment declares an engine
    // LightingV1 extension inside its disabled LIGHTING branch. This loader does
    // not load that dependency or silently accept an active extension.
    let selected = active_source(&source, &platform)?;
    let mut fragment = String::new();
    expand_source(
        assets,
        path,
        &selected,
        &mut Vec::new(),
        &mut BTreeSet::new(),
        &mut fragment,
    )?;
    let active_fragment = active_source(&fragment, &platform)?;
    let mut uniforms = BTreeMap::new();
    inspect(&active_fragment, &mut uniforms, &mut BTreeMap::new())?;
    uniforms.retain(|name, uniform| {
        uniform.kind != "sampler2D" || referenced_sampler(&active_fragment, name)
    });
    for uniform in uniforms.values() {
        let kind = match uniform.name.as_str() {
            "g_Texture0" => "sampler2D",
            "g_Overbright" | "g_CutoutStart" | "g_CutoutEnd" | "g_CutoutOpacity" => "float",
            other => bail!("unsupported_scene: particle shader uniform {other}"),
        };
        ensure!(
            !uniform.array && uniform.kind == kind,
            "unsupported_scene: particle shader uniform type {}",
            uniform.name
        );
    }
    for name in [
        "g_Texture0",
        "g_Overbright",
        "g_CutoutStart",
        "g_CutoutEnd",
        "g_CutoutOpacity",
    ] {
        ensure!(
            uniforms.contains_key(name),
            "unsupported_scene: missing particle shader uniform {name}"
        );
    }
    let vertex = if combos.get("SPRITESHEET").copied().unwrap_or(0) != 0 {
        "#version 130\nin vec2 a_Position;\nin vec2 a_TexCoord;\nin vec2 a_TexCoordNext;\nin float a_TexCoordBlend;\nin vec4 a_Color;\nuniform mat4 g_ModelViewProjectionMatrix;\nout vec4 v_TexCoord;\nout float v_TexCoordBlend;\nout vec4 v_Color;\nvoid main(){gl_Position=g_ModelViewProjectionMatrix*vec4(a_Position,0.0,1.0);v_TexCoord=vec4(a_TexCoord,a_TexCoordNext);v_TexCoordBlend=a_TexCoordBlend;v_Color=a_Color;}"
    } else {
        "#version 130\nin vec2 a_Position;\nin vec2 a_TexCoord;\nin vec4 a_Color;\nuniform mat4 g_ModelViewProjectionMatrix;\nout vec2 v_TexCoord;\nout vec4 v_Color;\nvoid main(){gl_Position=g_ModelViewProjectionMatrix*vec4(a_Position,0.0,1.0);v_TexCoord=a_TexCoord;v_Color=a_Color;}"
    };
    inspect(vertex, &mut uniforms, &mut BTreeMap::new())?;
    Ok(Shader {
        vertex: vertex.to_owned(),
        fragment: adapt(&fragment, false, &combos)?,
        uniforms: uniforms.into_values().collect(),
    })
}

fn inspect(
    source: &str,
    uniforms: &mut BTreeMap<String, Uniform>,
    combos: &mut BTreeMap<String, i32>,
) -> Result<()> {
    for line in source.lines() {
        if (line.contains("[COMBO]") || line.contains("[COMBO_OFF]"))
            && let Some(meta) = metadata(line)?
            && let Some(combo) = meta["combo"].as_str()
        {
            ensure!(
                identifier(combo),
                "unsupported_scene: invalid shader combo name"
            );
            let value = meta["default"].as_i64().unwrap_or(0);
            combos
                .entry(combo.to_owned())
                .or_insert(i32::try_from(value)?);
        }
        let Some(declaration) = line.trim().strip_prefix("uniform ") else {
            continue;
        };
        let Some((declaration, _)) = declaration.split_once(';') else {
            continue;
        };
        let mut parts = declaration.split_whitespace();
        let kind = parts.next().context("Missing uniform type")?;
        let name = parts.next().context("Missing uniform name")?;
        ensure!(
            parts.next().is_none(),
            "unsupported_scene: complex uniform declaration"
        );
        let array = name.contains('[');
        let array_length = name
            .split_once('[')
            .and_then(|(_, size)| size.strip_suffix(']'))
            .and_then(|size| size.parse().ok());
        let name = name.split('[').next().unwrap_or(name);
        ensure!(identifier(name), "unsupported_scene: invalid uniform name");
        let meta = metadata(line)?.unwrap_or(Value::Null);
        let material_range = meta
            .get("range")
            .map(|range| -> Result<[f32; 2]> {
                let range = range.as_array().context("Invalid shader material range")?;
                ensure!(range.len() == 2, "Invalid shader material range length");
                let minimum = range[0].as_f64().context("Invalid shader range minimum")? as f32;
                let maximum = range[1].as_f64().context("Invalid shader range maximum")? as f32;
                ensure!(
                    minimum.is_finite() && maximum.is_finite() && minimum <= maximum,
                    "Invalid shader material range bounds"
                );
                Ok([minimum, maximum])
            })
            .transpose()?;
        let uniform = Uniform {
            name: name.to_owned(),
            kind: kind.to_owned(),
            material: meta["material"].as_str().map(str::to_owned),
            default: meta.get("default").cloned(),
            texture_default: meta["default"]
                .as_str()
                .filter(|_| kind == "sampler2D")
                .map(str::to_owned),
            texture_combo: meta["combo"].as_str().map(str::to_owned),
            array,
            array_length,
            material_range,
        };
        if let Some(existing) = uniforms.get(name) {
            ensure!(
                existing.kind == uniform.kind
                    && existing.array == uniform.array
                    && existing.array_length == uniform.array_length,
                "unsupported_scene: conflicting uniform {name}"
            );
        } else {
            uniforms.insert(name.to_owned(), uniform);
        }
    }
    Ok(())
}

fn replace_identifiers(source: &str, vertex: bool) -> String {
    let mut result = String::with_capacity(source.len());
    let bytes = source.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor].is_ascii_alphabetic() || bytes[cursor] == b'_' {
            let start = cursor;
            cursor += 1;
            while cursor < bytes.len()
                && (bytes[cursor].is_ascii_alphanumeric() || bytes[cursor] == b'_')
            {
                cursor += 1;
            }
            let token = &source[start..cursor];
            result.push_str(match token {
                "attribute" => "in",
                "varying" if vertex => "out",
                "varying" => "in",
                "gl_FragColor" => "lwe_FragColor",
                other => other,
            });
        } else {
            let character = source[cursor..]
                .chars()
                .next()
                .expect("valid UTF-8 boundary");
            result.push(character);
            cursor += character.len_utf8();
        }
    }
    result
}

// Evaluate the bounded combo expressions used to gate shader declarations.
// GLSL itself still preprocesses the shader body; this only prevents loading
// defaults for disabled samplers (for example COPYBG's runtime framebuffer).
fn condition(expression: &str, values: &BTreeMap<String, i32>) -> Result<bool> {
    let expression = expression.trim();
    ensure!(
        expression.len() <= 512,
        "unsupported_scene: shader condition exceeds limit"
    );
    for operator in ["||", "&&", "==", "!=", ">=", "<=", ">", "<"] {
        let mut depth = 0_i32;
        for (index, character) in expression.char_indices() {
            if character == '(' {
                depth += 1;
            } else if character == ')' {
                depth -= 1;
            }
            if depth == 0 && expression[index..].starts_with(operator) {
                let left = &expression[..index];
                let right = &expression[index + operator.len()..];
                return match operator {
                    "||" => Ok(condition(left, values)? || condition(right, values)?),
                    "&&" => Ok(condition(left, values)? && condition(right, values)?),
                    "==" => Ok(condition_number(left, values)? == condition_number(right, values)?),
                    "!=" => Ok(condition_number(left, values)? != condition_number(right, values)?),
                    ">=" => Ok(condition_number(left, values)? >= condition_number(right, values)?),
                    "<=" => Ok(condition_number(left, values)? <= condition_number(right, values)?),
                    ">" => Ok(condition_number(left, values)? > condition_number(right, values)?),
                    "<" => Ok(condition_number(left, values)? < condition_number(right, values)?),
                    _ => unreachable!(),
                };
            }
        }
    }
    if let Some(inner) = expression.strip_prefix('!') {
        return Ok(!condition(inner, values)?);
    }
    if expression.starts_with('(') && expression.ends_with(')') {
        return condition(&expression[1..expression.len() - 1], values);
    }
    Ok(condition_number(expression, values)? != 0)
}

fn condition_number(expression: &str, values: &BTreeMap<String, i32>) -> Result<i32> {
    let expression = expression.trim();
    if let Ok(number) = expression.parse() {
        return Ok(number);
    }
    ensure!(
        identifier(expression),
        "unsupported_scene: unsupported shader condition {expression}"
    );
    Ok(values.get(expression).copied().unwrap_or(0))
}

fn shader_directive_source(source: &str) -> Result<String> {
    let mut bytes = source.as_bytes().to_vec();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if &bytes[i..i + 2] == b"//" {
            while i < bytes.len() && bytes[i] != b'\n' {
                bytes[i] = b' ';
                i += 1;
            }
        } else if &bytes[i..i + 2] == b"/*" {
            bytes[i] = b' ';
            bytes[i + 1] = b' ';
            i += 2;
            let mut closed = false;
            while i + 1 < bytes.len() {
                if &bytes[i..i + 2] == b"*/" {
                    bytes[i] = b' ';
                    bytes[i + 1] = b' ';
                    i += 2;
                    closed = true;
                    break;
                }
                if bytes[i] != b'\n' {
                    bytes[i] = b' ';
                }
                i += 1;
            }
            ensure!(closed, "Unterminated shader comment");
        } else {
            i += 1;
        }
    }
    Ok(String::from_utf8(bytes).expect("comment masking preserves UTF-8"))
}

fn macro_condition(
    expression: &str,
    values: &BTreeMap<String, i32>,
    definitions: &BTreeMap<String, String>,
) -> Result<bool> {
    fn resolve(
        name: &str,
        values: &BTreeMap<String, i32>,
        definitions: &BTreeMap<String, String>,
        depth: usize,
    ) -> Result<i32> {
        ensure!(depth < 32, "Shader conditional macro alias depth/cycle");
        let Some(expression) = definitions.get(name) else {
            return Ok(values.get(name).copied().unwrap_or(0));
        };
        let mut expression = expression.trim();
        while expression.starts_with('(') && expression.ends_with(')') {
            expression = expression[1..expression.len() - 1].trim();
        }
        if let Ok(value) = expression.parse::<i32>() {
            return Ok(value);
        }
        ensure!(
            identifier(expression),
            "Unsupported numeric conditional macro {name}"
        );
        resolve(expression, values, definitions, depth + 1)
    }
    let mut resolved = values.clone();
    for token in expression
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|t| identifier(t))
    {
        resolved.insert(token.to_owned(), resolve(token, values, definitions, 0)?);
    }
    condition(expression, &resolved)
}

fn active_source(source: &str, values: &BTreeMap<String, i32>) -> Result<String> {
    struct Branch {
        parent: bool,
        taken: bool,
        enabled: bool,
    }
    let mut branches: Vec<Branch> = Vec::new();
    let mut active = true;
    let mut result = String::new();
    let mut macros = values.clone();
    let mut definitions = BTreeMap::new();
    let directives = shader_directive_source(source)?;
    for (line, directive) in source.lines().zip(directives.lines()) {
        let directive = directive.trim();
        let if_result = if let Some(expression) = directive.strip_prefix("#if ") {
            Some(macro_condition(expression, &macros, &definitions)?)
        } else if let Some(name) = directive.strip_prefix("#ifdef ") {
            Some(macros.contains_key(name.trim()))
        } else {
            directive
                .strip_prefix("#ifndef ")
                .map(|name| !macros.contains_key(name.trim()))
        };
        if let Some(enabled) = if_result {
            ensure!(
                branches.len() < 64,
                "unsupported_scene: shader conditional nesting exceeds limit"
            );
            branches.push(Branch {
                parent: active,
                taken: enabled,
                enabled,
            });
            active = active && enabled;
        } else if let Some(expression) = directive.strip_prefix("#elif ") {
            let branch = branches.last_mut().context("Unmatched shader #elif")?;
            branch.enabled = !branch.taken && macro_condition(expression, &macros, &definitions)?;
            branch.taken |= branch.enabled;
            active = branch.parent && branch.enabled;
        } else if directive == "#else" {
            let branch = branches.last_mut().context("Unmatched shader #else")?;
            branch.enabled = !branch.taken;
            branch.taken = true;
            active = branch.parent && branch.enabled;
        } else if directive == "#endif" {
            active = branches.pop().context("Unmatched shader #endif")?.parent;
        } else if active {
            if let Some(definition) = directive.strip_prefix("#define ") {
                let mut parts = definition.splitn(2, char::is_whitespace);
                let name = parts.next().context("Missing shader macro name")?;
                let name = name.split('(').next().unwrap_or(name);
                ensure!(identifier(name), "Invalid shader macro name");
                let expression = parts.next().unwrap_or("0").trim();
                ensure!(
                    definitions.len() < 4096 || definitions.contains_key(name),
                    "Shader macro declaration budget exceeded"
                );
                definitions.insert(name.to_owned(), expression.to_owned());
                macros.insert(name.to_owned(), 0);
            } else if let Some(name) = directive.strip_prefix("#undef ") {
                ensure!(identifier(name.trim()), "Invalid shader macro undef");
                macros.remove(name.trim());
                definitions.remove(name.trim());
            }
            result.push_str(line);
            result.push('\n');
        }
    }
    ensure!(branches.is_empty(), "Unterminated shader conditional");
    Ok(result)
}

fn referenced_sampler(source: &str, name: &str) -> bool {
    source
        .lines()
        .filter(|line| !line.trim().starts_with("uniform "))
        .any(|line| {
            let code = line.split("//").next().unwrap_or("");
            code.split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
                .any(|token| token == name)
        })
}

fn adapt(source: &str, vertex: bool, combos: &BTreeMap<String, i32>) -> Result<String> {
    adapt_with_sampling(source, vertex, combos, false)
}

fn adapt_with_sampling(
    source: &str,
    vertex: bool,
    combos: &BTreeMap<String, i32>,
    legacy_static: bool,
) -> Result<String> {
    let mut result = String::from(
        "#version 130\n#define GLSL 1\n#define frac fract\n#define lerp mix\n#define texSample2D texture\n#define texSample2DLod textureLod\n#define CAST2 vec2\n#define CAST3 vec3\n#define CAST4 vec4\n#define CAST3X3 mat3\n#define CAST4X4 mat4\n#define CASTU uint\n#define saturate(x) clamp((x), 0.0, 1.0)\n#define mul(a,b) ((b)*(a))\n",
    );
    if legacy_static {
        result = result
            .replace(
                "#define texSample2D texture\n",
                "#define texSample2D lwe_LegacyTexture\n",
            )
            .replace(
                "#define texSample2DLod textureLod\n",
                "#define texSample2DLod lwe_LegacyTextureLod\n",
            );
        // Legacy card shaders use author coordinates with Y increasing from
        // the top. Static uploads in this renderer have their rows flipped.
        // Keep author math intact and convert only at the sampling boundary.
        result.push_str("vec4 lwe_LegacyTexture(sampler2D image, vec2 uv){return texture(image,vec2(uv.x,1.0-uv.y));}\nvec4 lwe_LegacyTextureLod(sampler2D image, vec2 uv, float lod){return textureLod(image,vec2(uv.x,1.0-uv.y),lod);}\n");
    }
    for (name, value) in combos {
        ensure!(identifier(name), "unsupported_scene: invalid combo name");
        ensure!(
            !matches!(name.as_str(), "HLSL" | "GLSL" | "HLSL_SM30"),
            "unsupported_scene: platform combo override"
        );
        result.push_str(&format!("#define {name} {value}\n"));
    }
    if !vertex {
        result.push_str("out vec4 lwe_FragColor;\n");
    }
    // Some shipped effect include files refer to samplers declared later in the
    // original file. Hoisting uniform declarations makes that valid desktop GLSL.
    let mut declarations = BTreeSet::new();
    for line in source.lines() {
        if line.trim().starts_with("uniform ") {
            let declaration = line
                .split(';')
                .next()
                .context("Invalid uniform declaration")?;
            declarations.insert(format!("{};\n", declaration.trim()));
        }
    }
    for declaration in declarations {
        result.push_str(&declaration);
    }
    let body = source
        .lines()
        .filter(|line| !line.trim().starts_with("uniform "))
        .collect::<Vec<_>>()
        .join("\n");
    result.push_str(&replace_identifiers(&body, vertex));
    Ok(result)
}

pub(crate) fn is_legacy_image(name: &str) -> bool {
    matches!(name, "genericimage" | "flowimage" | "swayimage" | "flag")
}

fn validate_legacy_combos(name: &str, values: &BTreeMap<String, i32>) -> Result<()> {
    ensure!(
        is_legacy_image(name),
        "unsupported_scene: unknown legacy image shader {name}"
    );
    for (combo, value) in values {
        let allowed = match (name, combo.as_str()) {
            ("genericimage", "MULTI" | "SPRITESHEET") => *value == 0,
            ("flag", "TINT") => matches!(*value, 0 | 1),
            ("flag", "TEX1FORMAT") => matches!(*value, 0 | 4),
            _ => false,
        };
        ensure!(
            allowed,
            "unsupported_scene: unvalidated {name} combo {combo}={value}"
        );
    }
    if name == "flag" {
        // Decoding BC3 to RGBA preserves the normal-map channel packing. The
        // caller derives this combo from the actual bound static texture.
        ensure!(
            values.contains_key("TEX1FORMAT"),
            "unsupported_scene: flag normal texture format was not supplied"
        );
    }
    Ok(())
}

fn legacy_uniform(name: &str, uniform: &str) -> Option<(&'static str, Option<&'static str>)> {
    match uniform {
        "g_ModelViewProjectionMatrix" => return Some(("mat4", None)),
        "g_Time" => return Some(("float", None)),
        "g_Texture0Resolution" => return Some(("vec4", None)),
        _ => {}
    }
    let material = match (name, uniform) {
        ("genericimage" | "flowimage" | "swayimage", "g_Brightness") => "Bright",
        ("genericimage", "g_UserAlpha") => "Alpha",
        ("genericimage", "g_Power") => "Power",
        ("genericimage", "g_ScrollX") => "Scroll 1 X",
        ("genericimage", "g_ScrollY") => "Scroll 1 Y",
        ("genericimage", "g_Texture0Rotation") => return Some(("vec4", None)),
        ("genericimage", "g_Texture0Translation") => return Some(("vec2", None)),
        ("flowimage", "g_FlowAmp") | ("swayimage", "g_SwayAmp") => "Amount",
        ("flowimage", "g_FlowSpeed") | ("swayimage", "g_SwaySpeed") => "Speed",
        ("flowimage", "g_FlowSpeed0") => "Speed0",
        ("flowimage", "g_FlowSpeed1") => "Speed1",
        ("flowimage", "g_FlowSpeed2") => "Speed2",
        ("flowimage", "g_FlowSpeed3") => "Speed3",
        ("flag", "g_WaveSpeed") => "Speed",
        ("flag", "g_WaveStrength") => "Strength",
        ("flag", "g_Color1") => return Some(("vec3", Some("color1"))),
        ("flag", "g_Color2") => return Some(("vec3", Some("color2"))),
        ("flag", "g_Color3") => return Some(("vec3", Some("color3"))),
        _ => return None,
    };
    Some(("float", Some(material)))
}

fn legacy_line(
    line: &str,
    texcoord: bool,
    block_comment: &mut bool,
    main_count: &mut usize,
) -> String {
    let mut result = String::with_capacity(line.len());
    let mut cursor = 0;
    let bytes = line.as_bytes();
    while cursor < bytes.len() {
        if !*block_comment && line[cursor..].starts_with("//") {
            result.push_str(&line[cursor..]);
            break;
        }
        if line[cursor..].starts_with(if *block_comment { "*/" } else { "/*" }) {
            *block_comment = !*block_comment;
            result.push_str(&line[cursor..cursor + 2]);
            cursor += 2;
        } else if !*block_comment && (bytes[cursor].is_ascii_alphabetic() || bytes[cursor] == b'_')
        {
            let start = cursor;
            cursor += 1;
            while cursor < bytes.len()
                && (bytes[cursor].is_ascii_alphanumeric() || bytes[cursor] == b'_')
            {
                cursor += 1;
            }
            result.push_str(match &line[start..cursor] {
                "a_TexCoord" if texcoord => "lwe_LegacyTexCoord",
                "main" => {
                    *main_count += 1;
                    "lwe_LegacyMain"
                }
                other => other,
            });
        } else {
            let character = line[cursor..].chars().next().expect("UTF-8 boundary");
            result.push(character);
            cursor += character.len_utf8();
        }
    }
    result
}

fn legacy_vertex(source: &str) -> Result<String> {
    let mut result = String::from("uniform vec4 g_Texture0Resolution;\nvec2 lwe_LegacyTexCoord;\n");
    let mut attributes = BTreeSet::new();
    let mut main_count = 0;
    let mut block_comment = false;
    for line in source.lines() {
        if let Some(declaration) = line.trim().strip_prefix("attribute ") {
            let declaration = declaration.split(';').next().unwrap_or("");
            let parts = declaration.split_whitespace().collect::<Vec<_>>();
            ensure!(
                matches!(
                    parts.as_slice(),
                    ["vec3", "a_Position"] | ["vec2", "a_TexCoord"]
                ),
                "unsupported_scene: legacy image vertex attribute"
            );
            ensure!(
                attributes.insert(parts[1]),
                "unsupported_scene: duplicate legacy image attribute"
            );
            result.push_str(line);
        } else if line.trim().starts_with("uniform ") && line.contains("g_Texture0Resolution") {
            // The validated declaration was already inserted above.
        } else {
            result.push_str(&legacy_line(
                line,
                true,
                &mut block_comment,
                &mut main_count,
            ));
        }
        result.push('\n');
    }
    ensure!(
        attributes == BTreeSet::from(["a_Position", "a_TexCoord"]) && main_count == 1,
        "unsupported_scene: legacy image vertex entry point or geometry"
    );
    result.push_str("void main(){lwe_LegacyTexCoord=vec2(a_TexCoord.x,1.0-a_TexCoord.y)*g_Texture0Resolution.zw/g_Texture0Resolution.xy;lwe_LegacyMain();}\n");
    Ok(result)
}

fn legacy_fragment(source: &str) -> Result<String> {
    let mut result = String::from("uniform vec4 g_Color4;\n");
    let mut main_count = 0;
    let mut block_comment = false;
    for line in source.lines() {
        result.push_str(&legacy_line(
            line,
            false,
            &mut block_comment,
            &mut main_count,
        ));
        result.push('\n');
    }
    ensure!(
        main_count == 1,
        "unsupported_scene: legacy image fragment entry point"
    );
    // Apply the object tint in its base material, before later effect stages.
    result.push_str("void main(){lwe_LegacyMain();gl_FragColor*=g_Color4;}\n");
    Ok(result)
}

/// Adapt the installed or project-owned source, without embedding its visual
/// math. Only the bounded legacy static-card material interface is supported.
/// The caller additionally validates that every sampler is a static texture.
pub(crate) fn load_legacy_image(
    assets: &SceneAssets,
    name: &str,
    overrides: &BTreeMap<String, i32>,
    bound_slots: &BTreeSet<usize>,
) -> Result<Shader> {
    validate_legacy_combos(name, overrides)?;
    let mut vertex = String::new();
    let mut fragment = String::new();
    for (extension, output) in [("vert", &mut vertex), ("frag", &mut fragment)] {
        expand(
            assets,
            &format!("shaders/{name}.{extension}"),
            &mut Vec::new(),
            &mut BTreeSet::new(),
            output,
        )?;
    }
    ensure!(
        !vertex.contains("lwe_Legacy") && !fragment.contains("lwe_Legacy"),
        "unsupported_scene: reserved legacy shader identifiers"
    );
    let mut combos = BTreeMap::new();
    inspect(&vertex, &mut BTreeMap::new(), &mut combos)?;
    inspect(&fragment, &mut BTreeMap::new(), &mut combos)?;
    combos.extend(overrides.clone());
    validate_legacy_combos(name, &combos)?;
    let mut platform = combos.clone();
    platform.insert("GLSL".to_owned(), 1);
    let active_vertex = active_source(&vertex, &platform)?;
    let active_fragment = active_source(&fragment, &platform)?;
    let mut uniforms = BTreeMap::new();
    inspect(&active_vertex, &mut uniforms, &mut BTreeMap::new())?;
    inspect(&active_fragment, &mut uniforms, &mut BTreeMap::new())?;
    let mut samplers = BTreeSet::new();
    uniforms.retain(|uniform, value| {
        value.kind != "sampler2D"
            || referenced_sampler(&active_vertex, uniform)
            || referenced_sampler(&active_fragment, uniform)
    });
    for uniform in uniforms.values() {
        ensure!(
            !uniform.array,
            "unsupported_scene: legacy image uniform array {}",
            uniform.name
        );
        if uniform.kind == "sampler2D" {
            let slot = uniform
                .name
                .strip_prefix("g_Texture")
                .and_then(|slot| slot.parse::<usize>().ok())
                .context("unsupported_scene: legacy sampler name")?;
            ensure!(
                uniform.texture_default.is_none() && uniform.texture_combo.is_none(),
                "unsupported_scene: legacy sampler dependency"
            );
            samplers.insert(slot);
        } else {
            let (kind, material) = legacy_uniform(name, &uniform.name).with_context(|| {
                format!("unsupported_scene: unknown {name} uniform {}", uniform.name)
            })?;
            ensure!(
                uniform.kind == kind && uniform.material.as_deref() == material,
                "unsupported_scene: legacy image uniform contract {}",
                uniform.name
            );
        }
    }
    let valid_samplers = match name {
        "genericimage" => samplers == BTreeSet::from([0]),
        "flowimage" => {
            samplers == BTreeSet::from([0, 1]) || samplers == BTreeSet::from([0, 1, 2, 3])
        }
        "swayimage" => samplers == BTreeSet::from([0, 1]),
        "flag" => samplers == BTreeSet::from([0, 1, 2]),
        _ => false,
    };
    ensure!(
        valid_samplers && &samplers == bound_slots,
        "unsupported_scene: legacy image sampler bindings"
    );
    ensure!(
        uniforms.contains_key("g_ModelViewProjectionMatrix"),
        "unsupported_scene: legacy image MVP missing"
    );
    uniforms.retain(|uniform, _| {
        referenced_sampler(&active_vertex, uniform) || referenced_sampler(&active_fragment, uniform)
    });
    uniforms.insert(
        "g_Texture0Resolution".to_owned(),
        Uniform {
            name: "g_Texture0Resolution".to_owned(),
            kind: "vec4".to_owned(),
            material: None,
            default: None,
            texture_default: None,
            texture_combo: None,
            array: false,
            array_length: None,
            material_range: None,
        },
    );
    uniforms.insert(
        "g_Color4".to_owned(),
        Uniform {
            name: "g_Color4".to_owned(),
            kind: "vec4".to_owned(),
            material: None,
            default: None,
            texture_default: None,
            texture_combo: None,
            array: false,
            array_length: None,
            material_range: None,
        },
    );
    Ok(Shader {
        vertex: adapt_with_sampling(&legacy_vertex(&active_vertex)?, true, &combos, true)?,
        fragment: adapt_with_sampling(&legacy_fragment(&fragment)?, false, &combos, true)?,
        uniforms: uniforms.into_values().collect(),
    })
}

/// Known scalar leftovers in shipped legacy materials. The caller still
/// validates their finite scalar type; this does not permit arbitrary keys.
pub(crate) fn legacy_constant_is_inert(name: &str, key: &str) -> bool {
    match name {
        "flowimage" => matches!(key, "Alpha" | "Power" | "Speed" | "Scroll 1 Y"),
        "swayimage" => matches!(key, "Power" | "Scroll 1 X"),
        _ => false,
    }
}

pub(crate) fn legacy_is_animated(
    name: &str,
    shader: &Shader,
    constants: &BTreeMap<String, Vec<f32>>,
) -> bool {
    if !is_legacy_image(name)
        || !shader
            .uniforms
            .iter()
            .any(|uniform| uniform.name == "g_Time")
    {
        return false;
    }
    name != "genericimage"
        || ["g_ScrollX", "g_ScrollY"].iter().any(|name| {
            constants
                .get(*name)
                .and_then(|value| value.first())
                .is_some_and(|value| *value != 0.0)
        })
}

/// The runtime interface accepted for static cards and project-owned 2D passes.
/// A material annotation is required for every other scalar/vector uniform.
pub(crate) fn runtime_uniform_kind(name: &str) -> Option<&'static str> {
    match name {
        "g_ModelViewProjectionMatrix" | "g_ModelMatrix" | "g_ModelViewProjectionMatrixInverse" => {
            Some("mat4")
        }
        "g_Time" => Some("float"),
        "g_Color4" | "g_Texture0UVTransform" => Some("vec4"),
        "g_TexelSize" | "g_PointerPosition" | "g_ParallaxPosition" => Some("vec2"),
        "g_AudioSpectrum16Left"
        | "g_AudioSpectrum16Right"
        | "g_AudioSpectrum32Left"
        | "g_AudioSpectrum32Right"
        | "g_AudioSpectrum64Left"
        | "g_AudioSpectrum64Right" => Some("float"),
        name if name
            .strip_prefix("g_Texture")
            .and_then(|name| name.strip_suffix("Resolution"))
            .and_then(|slot| slot.parse::<usize>().ok())
            .is_some_and(|slot| slot < 8) =>
        {
            Some("vec4")
        }
        _ => None,
    }
}

pub(crate) fn is_audio_spectrum(uniform: &Uniform) -> bool {
    let length = match uniform.name.as_str() {
        "g_AudioSpectrum16Left" | "g_AudioSpectrum16Right" => Some(16),
        "g_AudioSpectrum32Left" | "g_AudioSpectrum32Right" => Some(32),
        "g_AudioSpectrum64Left" | "g_AudioSpectrum64Right" => Some(64),
        _ => None,
    };
    length.is_some()
        && uniform.kind == "float"
        && uniform.array
        && uniform.array_length == length
        && uniform.material.is_none()
}

fn validate_bounded_uniforms(uniforms: &BTreeMap<String, Uniform>) -> Result<()> {
    for uniform in uniforms.values() {
        ensure!(
            (!uniform.array
                && !matches!(
                    uniform.name.as_str(),
                    "g_AudioSpectrum16Left"
                        | "g_AudioSpectrum16Right"
                        | "g_AudioSpectrum32Left"
                        | "g_AudioSpectrum32Right"
                        | "g_AudioSpectrum64Left"
                        | "g_AudioSpectrum64Right"
                ))
                || is_audio_spectrum(uniform),
            "unsupported_scene: unvalidated uniform array {}",
            uniform.name
        );
        if uniform.kind == "sampler2D" {
            ensure!(
                uniform
                    .name
                    .strip_prefix("g_Texture")
                    .and_then(|slot| slot.parse::<usize>().ok())
                    .is_some_and(|slot| slot < 8),
                "unsupported_scene: unvalidated 2D sampler {}",
                uniform.name
            );
        } else if uniform.material.is_some() {
            ensure!(
                matches!(uniform.kind.as_str(), "float" | "vec2" | "vec3" | "vec4"),
                "unsupported_scene: unvalidated material uniform {}",
                uniform.name
            );
        } else {
            ensure!(
                runtime_uniform_kind(&uniform.name) == Some(uniform.kind.as_str()),
                "unsupported_scene: unvalidated runtime uniform {}",
                uniform.name
            );
        }
    }
    Ok(())
}

fn validate_card_attributes(vertex: &str) -> Result<()> {
    let mut attributes = BTreeSet::new();
    for line in vertex.lines() {
        if let Some(declaration) = line.trim().strip_prefix("attribute ") {
            let declaration = declaration.split(';').next().unwrap_or("");
            let parts = declaration.split_whitespace().collect::<Vec<_>>();
            ensure!(
                matches!(
                    parts.as_slice(),
                    ["vec3", "a_Position"] | ["vec2", "a_TexCoord"]
                ),
                "unsupported_scene: unvalidated 2D shader vertex attribute"
            );
            ensure!(
                attributes.insert(parts[1]),
                "unsupported_scene: duplicate 2D vertex attribute"
            );
        }
    }
    ensure!(
        attributes == BTreeSet::from(["a_Position", "a_TexCoord"]),
        "unsupported_scene: incomplete 2D vertex attribute interface"
    );
    Ok(())
}

fn image2_combos(values: &BTreeMap<String, i32>, composite: bool) -> Result<()> {
    for (name, value) in values {
        let allowed = match name.as_str() {
            "VERSION" => matches!(*value, 1 | 2),
            "BLENDMODE" => *value == 0 || (composite && matches!(*value, 22 | 28 | 30 | 31)),
            "LIGHTING" | "REFLECTION" | "SPRITESHEET" | "PBRMASKS" | "SKINNING"
            | "SKINNING_ALPHA" | "HDR" | "PRELIGHTING" | "NORMALMAP" | "METALLIC_MAP"
            | "ROUGHNESS_MAP" | "REFLECTION_MAP" | "EMISSIVE_MAP" => *value == 0,
            _ => false,
        };
        ensure!(
            allowed,
            "unsupported_scene: genericimage2 combo {name}={value}"
        );
    }
    if composite {
        ensure!(
            values.get("VERSION") == Some(&2),
            "unsupported_scene: genericimage2 composition requires VERSION=2"
        );
    }
    Ok(())
}

fn image2_overrides(
    values: &BTreeMap<String, i32>,
    composite: bool,
) -> Result<BTreeMap<String, i32>> {
    let mut normalized = BTreeMap::new();
    for (name, value) in values {
        // Authored material keys may be lowercase while the installed shader
        // uses uppercase macros. Aliases must not silently win over one another.
        let name = name.to_ascii_uppercase();
        ensure!(
            normalized.insert(name.clone(), *value).is_none(),
            "unsupported_scene: ambiguous genericimage2 combo {name}"
        );
    }
    image2_combos(&normalized, composite)?;
    Ok(normalized)
}

struct ImageSources {
    vertex: String,
    fragment: String,
    combos: BTreeMap<String, i32>,
    uniforms: BTreeMap<String, Uniform>,
}

fn image2_sources(
    assets: &SceneAssets,
    overrides: &BTreeMap<String, i32>,
    composite: bool,
) -> Result<ImageSources> {
    let overrides = image2_overrides(overrides, composite)?;
    let mut vertex = String::new();
    let mut fragment = String::new();
    for (extension, output) in [("vert", &mut vertex), ("frag", &mut fragment)] {
        expand(
            assets,
            &format!("shaders/genericimage2.{extension}"),
            &mut Vec::new(),
            &mut BTreeSet::new(),
            output,
        )?;
    }
    let mut values = BTreeMap::new();
    inspect(&vertex, &mut BTreeMap::new(), &mut values)?;
    inspect(&fragment, &mut BTreeMap::new(), &mut values)?;
    values.extend(overrides);
    image2_combos(&values, composite)?;
    let mut platform = values.clone();
    platform.insert("GLSL".to_owned(), 1);
    let vertex = active_source(&vertex, &platform)?;
    let fragment = active_source(&fragment, &platform)?;
    validate_card_attributes(&vertex)?;
    let mut uniforms = BTreeMap::new();
    inspect(&vertex, &mut uniforms, &mut BTreeMap::new())?;
    inspect(&fragment, &mut uniforms, &mut BTreeMap::new())?;
    // The installed vertex keeps spritesheet declarations outside its disabled
    // branch. They are unused in this bounded static-card configuration.
    uniforms
        .retain(|name, _| referenced_sampler(&vertex, name) || referenced_sampler(&fragment, name));
    validate_bounded_uniforms(&uniforms)?;
    ensure!(
        uniforms.contains_key("g_ModelViewProjectionMatrix"),
        "unsupported_scene: genericimage2 card matrix interface"
    );
    // VERSION is an existence check in the installed source. Leaving it
    // undefined preserves the older material's independent brightness/alpha
    // equations; defining either verified value selects the newer layer tint.
    if values.contains_key("VERSION") {
        ensure!(
            uniforms
                .get("g_Color4")
                .is_some_and(|uniform| { uniform.kind == "vec4" && uniform.material.is_none() })
                && !uniforms.contains_key("g_Brightness")
                && !uniforms.contains_key("g_UserAlpha"),
            "unsupported_scene: genericimage2 tint interface"
        );
    } else {
        for (name, material) in [("g_Brightness", "Brightness"), ("g_UserAlpha", "Alpha")] {
            ensure!(
                uniforms.get(name).is_some_and(|uniform| {
                    uniform.kind == "float" && uniform.material.as_deref() == Some(material)
                }),
                "unsupported_scene: genericimage2 legacy material interface {name}"
            );
        }
        ensure!(
            !uniforms.contains_key("g_Color4"),
            "unsupported_scene: genericimage2 legacy tint interface"
        );
    }
    Ok(ImageSources {
        vertex,
        fragment,
        combos: values,
        uniforms,
    })
}

/// Load the original static-card equations. Author UVs are converted only at
/// the static upload sampling boundary; authored color equations stay intact.
pub(crate) fn load_card_image(
    assets: &SceneAssets,
    name: &str,
    overrides: &BTreeMap<String, i32>,
    bound_slots: &BTreeSet<usize>,
) -> Result<Shader> {
    ensure!(
        name == "genericimage2",
        "unsupported_scene: static card shader {name}"
    );
    ensure!(
        bound_slots == &BTreeSet::from([0]),
        "unsupported_scene: static card bindings"
    );
    let ImageSources {
        vertex,
        fragment,
        combos: values,
        mut uniforms,
    } = image2_sources(assets, overrides, false)?;
    ensure!(
        uniforms
            .values()
            .filter(|uniform| uniform.kind == "sampler2D")
            .all(|uniform| uniform.name == "g_Texture0"),
        "unsupported_scene: static card sampler interface"
    );
    let vertex = legacy_vertex(&vertex)?;
    // Older materials have no g_Color4 in their authored branch. Keep their
    // brightness/alpha math, then apply the modeled layer color exactly once.
    let fragment = if values.contains_key("VERSION") {
        fragment
    } else {
        legacy_fragment(&fragment)?
    };
    inspect(&vertex, &mut uniforms, &mut BTreeMap::new())?;
    inspect(&fragment, &mut uniforms, &mut BTreeMap::new())?;
    uniforms
        .retain(|name, _| referenced_sampler(&vertex, name) || referenced_sampler(&fragment, name));
    validate_bounded_uniforms(&uniforms)?;
    Ok(Shader {
        vertex: adapt_with_sampling(&vertex, true, &values, true)?,
        fragment: adapt_with_sampling(&fragment, false, &values, true)?,
        uniforms: uniforms.into_values().collect(),
    })
}

/// Final composition samples the completed layer and scene FBOs in native GL
/// coordinates. Blending therefore remains after the authored effect chain.
pub(crate) fn load_image_composite(assets: &SceneAssets, mode: i32) -> Result<Shader> {
    let values = BTreeMap::from([("VERSION".to_owned(), 2), ("BLENDMODE".to_owned(), mode)]);
    let ImageSources {
        vertex,
        fragment,
        combos: values,
        uniforms,
    } = image2_sources(assets, &values, true)?;
    Ok(Shader {
        vertex: adapt(&vertex, true, &values)?,
        fragment: adapt(&fragment, false, &values)?,
        uniforms: uniforms.into_values().collect(),
    })
}

/// These are general project shader interfaces, not Workshop item IDs.
pub(crate) fn is_bounded_project_effect(name: &str) -> bool {
    name.starts_with("workshop/")
        || matches!(
            name,
            "effects/motionblur_accumulation" | "effects/motionblur_combine"
        )
}

#[derive(Clone, Copy)]
struct SourceToken<'a> {
    start: usize,
    end: usize,
    value: &'a str,
}

fn source_tokens(source: &str) -> Result<Vec<SourceToken<'_>>> {
    let mut result = Vec::new();
    let mut position = 0;
    while position < source.len() {
        let rest = &source[position..];
        if rest.starts_with("//") {
            position += rest.find('\n').unwrap_or(rest.len());
        } else if let Some(comment) = rest.strip_prefix("/*") {
            position += comment.find("*/").context("Unterminated shader comment")? + 4;
        } else {
            let character = rest.chars().next().expect("UTF-8 boundary");
            if character.is_whitespace() {
                position += character.len_utf8();
                continue;
            }
            let start = position;
            position += character.len_utf8();
            if character.is_ascii_alphabetic() || character == '_' {
                while position < source.len()
                    && (source.as_bytes()[position].is_ascii_alphanumeric()
                        || source.as_bytes()[position] == b'_')
                {
                    position += 1;
                }
            }
            result.push(SourceToken {
                start,
                end: position,
                value: &source[start..position],
            });
            ensure!(
                result.len() <= 100_000,
                "unsupported_scene: shader token limit"
            );
        }
    }
    Ok(result)
}

// Wallpaper Engine's shared shader dialect permits HLSL implicit initializer
// casts. Apply explicit GLSL constructors to those declarations, including
// integer loop starts. This does not substitute any authored visual formula.
fn explicit_initializers(source: &str) -> Result<String> {
    let tokens = source_tokens(source)?;
    let mut edits = Vec::new();
    for index in 0..tokens.len().saturating_sub(3) {
        let token = tokens[index];
        if !matches!(
            token.value,
            "float" | "int" | "uint" | "vec2" | "vec3" | "vec4"
        ) || !identifier(tokens[index + 1].value)
            || tokens[index + 2].value != "="
        {
            continue;
        }
        let mut depth = 0_i32;
        let mut end = None;
        for current in &tokens[index + 3..] {
            match current.value {
                "(" | "[" => depth += 1,
                ")" | "]" => depth -= 1,
                ";" | "," if depth == 0 => {
                    end = Some(current.start);
                    break;
                }
                _ => {}
            }
        }
        let end = end.context("unsupported_scene: shader initializer syntax")?;
        let start = tokens[index + 3].start;
        edits.push((
            start,
            end,
            format!("{}({})", token.value, &source[start..end]),
        ));
        // GLSL 1.30 forbids runtime texture reads in const initializers, while
        // the shared dialect treats local const as an ordinary readonly local.
        if index > 0 && tokens[index - 1].value == "const" {
            edits.push((
                tokens[index - 1].start,
                tokens[index - 1].end,
                String::new(),
            ));
        }
    }
    let mut result = source.to_owned();
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.0));
    for (start, end, replacement) in edits {
        result.replace_range(start..end, &replacement);
    }
    Ok(result)
}

fn adapt_shared_dialect(
    source: &str,
    vertex: bool,
    combos: &BTreeMap<String, i32>,
) -> Result<String> {
    let mut source = explicit_initializers(source)?;
    let tokens = source_tokens(&source)?;
    let mut edits = Vec::new();
    for token in tokens {
        if let Some(replacement) = match token.value {
            "texSample2D" => Some("lwe_SharedTexture"),
            "pow" => Some("lwe_SharedPow"),
            _ => None,
        } {
            edits.push((token.start, token.end, replacement));
        }
    }
    for (start, end, replacement) in edits.into_iter().rev() {
        source.replace_range(start..end, replacement);
    }
    let helpers = "vec4 lwe_SharedTexture(sampler2D image, vec2 uv){return texture(image,uv);}\nvec4 lwe_SharedTexture(sampler2D image, vec3 uv){return texture(image,uv.xy);}\nvec4 lwe_SharedTexture(sampler2D image, vec4 uv){return texture(image,uv.xy);}\nfloat lwe_SharedPow(float x,float y){return pow(x,y);}\nvec2 lwe_SharedPow(vec2 x,vec2 y){return pow(x,y);}\nvec3 lwe_SharedPow(vec3 x,vec3 y){return pow(x,y);}\nvec4 lwe_SharedPow(vec4 x,vec4 y){return pow(x,y);}\nvec2 lwe_SharedPow(vec2 x,float y){return pow(x,vec2(y));}\nvec3 lwe_SharedPow(vec3 x,float y){return pow(x,vec3(y));}\nvec4 lwe_SharedPow(vec4 x,float y){return pow(x,vec4(y));}\n";
    adapt(&format!("{helpers}{source}"), vertex, combos)
}

fn load_bounded_pass(
    assets: &SceneAssets,
    name: &str,
    overrides: &BTreeMap<String, i32>,
    bound_slots: &BTreeSet<usize>,
) -> Result<Shader> {
    let mut vertex = String::new();
    let mut fragment = String::new();
    for (extension, output) in [("vert", &mut vertex), ("frag", &mut fragment)] {
        expand(
            assets,
            &format!("shaders/{name}.{extension}"),
            &mut Vec::new(),
            &mut BTreeSet::new(),
            output,
        )?;
    }
    ensure!(
        !vertex.contains("lwe_") && !fragment.contains("lwe_"),
        "unsupported_scene: reserved shader identifiers"
    );
    let mut uniforms = BTreeMap::new();
    let mut combos = BTreeMap::new();
    inspect(&vertex, &mut uniforms, &mut combos)?;
    inspect(&fragment, &mut uniforms, &mut combos)?;
    for uniform in uniforms.values() {
        if let Some(combo) = &uniform.texture_combo {
            let slot = uniform
                .name
                .strip_prefix("g_Texture")
                .and_then(|slot| slot.parse::<usize>().ok());
            combos.entry(combo.clone()).or_insert(i32::from(
                slot.is_some_and(|slot| bound_slots.contains(&slot)),
            ));
        }
    }
    combos.extend(overrides.clone());
    validate_effect_combos(name, &combos)?;
    let mut platform = combos.clone();
    platform.insert("GLSL".to_owned(), 1);
    let vertex = active_source(&vertex, &platform)?;
    let fragment = active_source(&fragment, &platform)?;
    validate_card_attributes(&vertex)?;
    let mut uniforms = BTreeMap::new();
    inspect(&vertex, &mut uniforms, &mut BTreeMap::new())?;
    inspect(&fragment, &mut uniforms, &mut BTreeMap::new())?;
    uniforms.retain(|name, uniform| {
        uniform.kind != "sampler2D"
            || referenced_sampler(&vertex, name)
            || referenced_sampler(&fragment, name)
    });
    validate_bounded_uniforms(&uniforms)?;
    Ok(Shader {
        vertex: adapt_shared_dialect(&vertex, true, &combos)?,
        fragment: adapt_shared_dialect(&fragment, false, &combos)?,
        uniforms: uniforms.into_values().collect(),
    })
}

pub(crate) fn load(
    assets: &SceneAssets,
    name: &str,
    overrides: &BTreeMap<String, i32>,
    bound_slots: &BTreeSet<usize>,
) -> Result<Shader> {
    validate_effect_combos(name, overrides)?;
    if matches!(name, "passthrough" | "effects/scroll" | "effects/pulse")
        || is_bounded_project_effect(name)
    {
        return load_bounded_pass(assets, name, overrides, bound_slots);
    }
    let mut vertex = String::new();
    let mut fragment = String::new();
    expand(
        assets,
        &format!("shaders/{name}.vert"),
        &mut Vec::new(),
        &mut BTreeSet::new(),
        &mut vertex,
    )?;
    expand(
        assets,
        &format!("shaders/{name}.frag"),
        &mut Vec::new(),
        &mut BTreeSet::new(),
        &mut fragment,
    )?;
    let mut uniforms = BTreeMap::new();
    let mut combos = BTreeMap::new();
    inspect(&vertex, &mut uniforms, &mut combos)?;
    inspect(&fragment, &mut uniforms, &mut combos)?;
    for uniform in uniforms.values() {
        if let Some(combo) = &uniform.texture_combo {
            let slot = uniform
                .name
                .strip_prefix("g_Texture")
                .and_then(|slot| slot.parse::<usize>().ok());
            combos.entry(combo.clone()).or_insert(i32::from(
                slot.is_some_and(|slot| bound_slots.contains(&slot)),
            ));
        }
    }
    combos.extend(overrides.clone());
    validate_effect_combos(name, &combos)?;
    ensure!(
        combos.get("AUDIOPROCESSING").copied().unwrap_or(0) == 0,
        "unsupported_scene: audio-reactive shader"
    );
    ensure!(
        combos.get("COPYBG").copied().unwrap_or(0) == 0,
        "unsupported_scene: background-copy shader"
    );
    let mut active_uniforms = BTreeMap::new();
    let mut platform_values = combos.clone();
    platform_values.insert("GLSL".to_owned(), 1);
    let active_vertex = active_source(&vertex, &platform_values)?;
    let active_fragment = active_source(&fragment, &platform_values)?;
    inspect(&active_vertex, &mut active_uniforms, &mut BTreeMap::new())?;
    inspect(&active_fragment, &mut active_uniforms, &mut BTreeMap::new())?;
    active_uniforms.retain(|name, uniform| {
        uniform.kind != "sampler2D"
            || referenced_sampler(&active_vertex, name)
            || referenced_sampler(&active_fragment, name)
    });
    Ok(Shader {
        vertex: adapt(&vertex, true, &combos)?,
        fragment: adapt(&fragment, false, &combos)?,
        uniforms: active_uniforms.into_values().collect(),
    })
}

// These effects are adapted from the project's own shader assets. Keep the
// approved macro surface bounded: perspective WaterRipple requires a separate
// geometric validation path and is not enabled by accepting its shader name.
fn validate_effect_combos(name: &str, overrides: &BTreeMap<String, i32>) -> Result<()> {
    if matches!(name, "passthrough" | "effects/scroll" | "effects/pulse")
        || is_bounded_project_effect(name)
    {
        for (combo, value) in overrides {
            let allowed = match (name, combo.as_str()) {
                ("passthrough", "SPRITESHEET") => *value == 0,
                ("passthrough", "TRANSFORM") => *value == 1,
                ("effects/scroll", _) => false,
                ("effects/motionblur_accumulation", "MASK") => matches!(*value, 0 | 1),
                (name, "RESOLUTION") if is_bounded_project_effect(name) => {
                    matches!(*value, 16 | 32 | 64)
                }
                (name, "SHAPE") if is_bounded_project_effect(name) => (0..=9).contains(value),
                (name, "TRANSPARENCY") if is_bounded_project_effect(name) => {
                    (0..=5).contains(value)
                }
                (name, "ANTIALIAS" | "CLIP_LOW" | "CLIP_HIGH" | "A_SMOOTH_CURVE")
                    if is_bounded_project_effect(name) =>
                {
                    matches!(*value, 0 | 1)
                }
                ("effects/pulse", "BLENDMODE") => (0..=31).contains(value),
                ("effects/pulse", "AUDIOPROCESSING") => (0..=3).contains(value),
                ("effects/pulse", "PULSEALPHA" | "PULSECOLOR" | "MASK") => matches!(*value, 0 | 1),
                (name, "BLENDMODE") if is_bounded_project_effect(name) => (0..=31).contains(value),
                (name, "AUDIOPROCESSING") if is_bounded_project_effect(name) => {
                    (0..=3).contains(value)
                }
                (
                    name,
                    "VERTICAL" | "MODE" | "INVERT" | "DAMP" | "OPACITY" | "MASK" | "EMITTERMASK",
                ) if is_bounded_project_effect(name) => matches!(*value, 0 | 1),
                _ => false,
            };
            ensure!(
                allowed,
                "unsupported_scene: unvalidated {name} combo {combo}={value}"
            );
        }
        return Ok(());
    }
    let allowed: &[&str] = match name {
        "effects/waterripple" => &["MASK", "SPECULAR", "PERSPECTIVE"],
        "effects/iris" => &["MASK", "BACKGROUND"],
        _ => return Ok(()),
    };
    for (combo, value) in overrides {
        ensure!(
            allowed.contains(&combo.as_str()) && matches!(*value, 0 | 1),
            "unsupported_scene: unvalidated {name} combo {combo}"
        );
    }
    ensure!(
        name != "effects/waterripple" || overrides.get("PERSPECTIVE").copied().unwrap_or(0) == 0,
        "unsupported_scene: perspective WaterRipple geometry"
    );
    Ok(())
}

pub(crate) fn image() -> Shader {
    Shader {
        vertex: "#version 130\nin vec3 a_Position; in vec2 a_TexCoord; out vec2 v_UV; uniform mat4 g_ModelViewProjectionMatrix; void main(){ gl_Position=g_ModelViewProjectionMatrix*vec4(a_Position,1.0); v_UV=a_TexCoord; }".into(),
        fragment: "#version 130\nin vec2 v_UV; out vec4 lwe_FragColor; uniform sampler2D g_Texture0; uniform vec4 g_Texture0Resolution; uniform vec4 g_Color4; void main(){ vec2 uv=v_UV*g_Texture0Resolution.zw/g_Texture0Resolution.xy; uv.y+=1.0-g_Texture0Resolution.w/g_Texture0Resolution.y; lwe_FragColor=texture(g_Texture0,uv)*g_Color4; }".into(),
        uniforms: Vec::new(),
    }
}

/// Uniform-color quad; vertex colors have a separate attribute/data contract.
pub(crate) fn flat_material(combos: &BTreeMap<String, i32>) -> Result<Shader> {
    ensure!(
        combos.is_empty(),
        "unsupported_scene: flat vertex/combo attributes are not modeled"
    );
    let mut shader = image();
    shader.fragment = "#version 130\nuniform vec4 g_Color4; out vec4 lwe_FragColor; void main(){lwe_FragColor=g_Color4;}".into();
    Ok(shader)
}

pub(crate) fn image_material() -> Shader {
    let mut shader = image();
    shader.fragment="#version 130\nin vec2 v_UV; out vec4 lwe_FragColor; uniform sampler2D g_Texture0; uniform sampler2D g_Texture1; uniform vec4 g_Texture0Resolution; uniform vec4 g_Color4; uniform int g_BlendMode; void main(){ vec2 uv=v_UV*g_Texture0Resolution.zw/g_Texture0Resolution.xy; uv.y+=1.0-g_Texture0Resolution.w/g_Texture0Resolution.y; vec4 src=texture(g_Texture0,uv)*g_Color4; vec4 dst=texture(g_Texture1,v_UV); if(g_BlendMode==0){lwe_FragColor=src;}else{float alpha=min(1.0,src.a+dst.a*(g_BlendMode==1?1.0-src.a:1.0)); vec3 premultiplied=src.rgb*src.a+dst.rgb*dst.a*(g_BlendMode==1?1.0-src.a:1.0); lwe_FragColor=vec4(alpha>0.0?premultiplied/alpha:vec3(0.0),alpha);} }".into();
    shader
}

/// The unlit image4 slice has one albedo and layer tint. A timed source is
/// exposed as a bounded physical region; its worker-selected virtual frame
/// transform replaces the native sprite rotation/translation at sampling.
pub(crate) fn image_material_variant(
    combos: &BTreeMap<String, i32>,
    timed: bool,
) -> Result<Shader> {
    for (name, value) in combos {
        let supported = match name.as_str() {
            "version" | "VERSION" => matches!(*value, 1 | 2),
            "SPRITESHEET" => matches!(*value, 0 | 1),
            "LIGHTING" | "REFLECTION" | "SKINNING" | "MORPHING" | "PBRMASKS" | "NORMALMAP"
            | "VERTEXCOLOR" | "BLENDMODE" | "CLIPPINGUVS" | "CLIPPINGTARGET"
            | "CLIPPINGCOMPOSE" | "FOG_DIST" | "FOG_HEIGHT" | "HDR" | "ALPHATOCOVERAGE" => {
                *value == 0
            }
            "FOG" => matches!(*value, 0 | 1),
            _ => *value == 0,
        };
        ensure!(
            supported,
            "unsupported_scene: genericimage4 combo {name}={value}"
        );
    }
    if let (Some(lower), Some(upper)) = (combos.get("version"), combos.get("VERSION")) {
        ensure!(
            lower == upper,
            "unsupported_scene: conflicting image4 versions"
        );
    }
    ensure!(
        (combos.get("SPRITESHEET").copied().unwrap_or(0) != 0) == timed,
        "unsupported_scene: image4 sprite branch and timed source disagree"
    );
    let mut shader = image_material();
    if timed {
        shader.fragment = shader.fragment.replace(
            "vec2 uv=v_UV*g_Texture0Resolution.zw/g_Texture0Resolution.xy; uv.y+=1.0-g_Texture0Resolution.w/g_Texture0Resolution.y;",
            "vec2 uv=v_UV*g_Texture0UVTransform.xy+g_Texture0UVTransform.zw;",
        ).replace(
            "uniform int g_BlendMode;",
            "uniform int g_BlendMode; uniform vec4 g_Texture0UVTransform;",
        );
        shader.uniforms.push(Uniform {
            name: "g_Texture0UVTransform".into(),
            kind: "vec4".into(),
            material: None,
            default: None,
            texture_default: None,
            texture_combo: None,
            array: false,
            array_length: None,
            material_range: None,
        });
    }
    Ok(shader)
}

pub(crate) fn text_material() -> Result<Shader> {
    image_material_variant(&BTreeMap::from([("SPRITESHEET".into(), 1)]), true)
}

/// Only complete LWE-owned templates and their canonical metadata qualify.
/// Keep authored effects and their integer uniforms unchanged.
pub(crate) fn specialize_owned_image_blend(shader: &Shader, mode: i32) -> Option<Shader> {
    if !(0..=2).contains(&mode)
        || (shader != &image_material() && shader != &text_material().ok()?)
    {
        return None;
    }
    let mut variant = shader.clone();
    variant.fragment = variant.fragment.replacen(
        "uniform int g_BlendMode;",
        &format!("const int g_BlendMode = {mode};"),
        1,
    );
    Some(variant)
}

#[cfg(test)]
#[path = "shader_owned_blend_tests.rs"]
mod owned_blend_tests;

#[cfg(test)]
mod tests {
    use super::*;

    struct LegacyFixture(std::path::PathBuf);

    impl LegacyFixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "lwe-legacy-shader-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            ));
            std::fs::create_dir_all(path.join("shaders")).unwrap();
            Self(path)
        }

        fn write(&self, name: &str, vertex: &str, fragment: &str) {
            if let Some(parent) = self.0.join(format!("shaders/{name}.vert")).parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(self.0.join(format!("shaders/{name}.vert")), vertex).unwrap();
            std::fs::write(self.0.join(format!("shaders/{name}.frag")), fragment).unwrap();
        }

        fn load(
            &self,
            name: &str,
            combos: &BTreeMap<String, i32>,
            slots: &[usize],
        ) -> Result<Shader> {
            load_legacy_image(
                &SceneAssets::open(&self.0, None)?,
                name,
                combos,
                &slots.iter().copied().collect(),
            )
        }
    }

    impl Drop for LegacyFixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    // These small shaders are authored test data, not copies of installed media
    // or the original visual math. They exercise the declared CPU interfaces.
    const LEGACY_VERTEX: &str = "uniform mat4 g_ModelViewProjectionMatrix;\nuniform float g_Time;\nattribute vec3 a_Position;\nattribute vec2 a_TexCoord;\nvarying vec2 v_TexCoord;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);v_TexCoord=a_TexCoord+vec2(0.0,g_Time*0.001);}\n";
    const LEGACY_GENERIC_FRAGMENT: &str = "uniform sampler2D g_Texture0;\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord);}\n";

    #[test]
    fn legacy_card_wrapper_preserves_real_attributes_and_author_y_math() {
        let fixture = LegacyFixture::new();
        fixture.write(
            "genericimage",
            &format!("{LEGACY_VERTEX}// main is an entry point; a_TexCoordExtra stays distinct\n"),
            LEGACY_GENERIC_FRAGMENT,
        );
        let shader = fixture
            .load("genericimage", &BTreeMap::new(), &[0])
            .unwrap();
        assert!(shader.vertex.contains("in vec2 a_TexCoord;"));
        assert!(
            shader
                .vertex
                .contains("v_TexCoord=lwe_LegacyTexCoord+vec2(0.0,g_Time*0.001)")
        );
        assert!(shader.vertex.contains(
            "vec2(a_TexCoord.x,1.0-a_TexCoord.y)*g_Texture0Resolution.zw/g_Texture0Resolution.xy"
        ));
        assert_eq!(shader.vertex.matches("void main()").count(), 1);
        assert_eq!(shader.vertex.matches("void lwe_LegacyMain()").count(), 1);
        assert!(shader.vertex.contains("a_TexCoordExtra"));
        assert!(
            shader.fragment.contains(
                "void lwe_LegacyMain(){lwe_FragColor=texSample2D(g_Texture0,v_TexCoord);}"
            )
        );
        assert!(
            shader
                .fragment
                .contains("void main(){lwe_LegacyMain();lwe_FragColor*=g_Color4;}")
        );
        assert_eq!(shader.fragment.matches("void main()").count(), 1);
        assert_eq!(
            shader
                .uniforms
                .iter()
                .filter(|uniform| uniform.name == "g_Color4")
                .count(),
            1
        );
        assert!(
            shader
                .fragment
                .contains("#define texSample2D lwe_LegacyTexture")
        );
        assert!(
            shader
                .fragment
                .contains("texture(image,vec2(uv.x,1.0-uv.y))")
        );
        assert!(
            shader
                .fragment
                .contains("textureLod(image,vec2(uv.x,1.0-uv.y),lod)")
        );
        assert_eq!(
            shader
                .uniforms
                .iter()
                .filter(|uniform| uniform.name == "g_Texture0Resolution")
                .count(),
            1
        );
        // The ordinary effect/FBO translation retains its existing convention.
        assert!(
            !adapt(LEGACY_GENERIC_FRAGMENT, false, &BTreeMap::new())
                .unwrap()
                .contains("lwe_LegacyTexture")
        );
    }

    #[test]
    fn flow_sampler_contract_comes_from_active_project_source() {
        let fixture = LegacyFixture::new();
        for slots in [2, 4] {
            let mut fragment = String::from("varying vec2 v_TexCoord;\n");
            for slot in 0..7 {
                fragment.push_str(&format!("uniform sampler2D g_Texture{slot};\n"));
            }
            fragment.push_str("void main(){vec4 color=vec4(0);\n");
            for slot in 0..slots {
                fragment.push_str(&format!(
                    "color+=texSample2D(g_Texture{slot},v_TexCoord);\n"
                ));
            }
            fragment.push_str("gl_FragColor=color;}\n");
            fixture.write("flowimage", LEGACY_VERTEX, &fragment);
            let indices = (0..slots).collect::<Vec<_>>();
            let shader = fixture
                .load("flowimage", &BTreeMap::new(), &indices)
                .unwrap();
            assert_eq!(
                shader
                    .uniforms
                    .iter()
                    .filter(|uniform| uniform.kind == "sampler2D")
                    .count(),
                slots
            );
            assert!(fixture.load("flowimage", &BTreeMap::new(), &[0]).is_err());
            assert!(
                fixture
                    .load("flowimage", &BTreeMap::new(), &[0, 1, 2, 3, 4])
                    .is_err()
            );
            assert!(legacy_is_animated("flowimage", &shader, &BTreeMap::new()));
        }
    }

    #[test]
    fn legacy_loader_retains_project_shader_precedence() {
        let project = LegacyFixture::new();
        let builtin = LegacyFixture::new();
        project.write(
            "genericimage",
            LEGACY_VERTEX,
            &format!("{LEGACY_GENERIC_FRAGMENT}// project shader marker\n"),
        );
        builtin.write(
            "genericimage",
            LEGACY_VERTEX,
            &format!("{LEGACY_GENERIC_FRAGMENT}// builtin shader marker\n"),
        );
        let assets = SceneAssets::open(&project.0, Some(&builtin.0)).unwrap();
        let shader = load_legacy_image(
            &assets,
            "genericimage",
            &BTreeMap::new(),
            &BTreeSet::from([0]),
        )
        .unwrap();
        assert!(shader.fragment.contains("project shader marker"));
        assert!(!shader.fragment.contains("builtin shader marker"));
    }

    #[test]
    fn legacy_combos_reject_geometry_platform_and_unknown_features() {
        for (name, combo, value) in [
            ("genericimage", "MULTI", 1),
            ("genericimage", "SPRITESHEET", 1),
            ("flowimage", "COPYBG", 0),
            ("swayimage", "TEX0FORMAT", 0),
            ("flag", "TINT", 2),
            ("flag", "TEX1FORMAT", 8),
            ("flag", "GLSL", 1),
            ("flag", "UNKNOWN", 0),
        ] {
            assert!(
                validate_legacy_combos(name, &BTreeMap::from([(combo.to_owned(), value)])).is_err()
            );
        }
        assert!(validate_legacy_combos("flag", &BTreeMap::new()).is_err());
        assert!(validate_legacy_combos("genericimage2", &BTreeMap::new()).is_err());
        for format in [0, 4] {
            validate_legacy_combos(
                "flag",
                &BTreeMap::from([("TINT".into(), 1), ("TEX1FORMAT".into(), format)]),
            )
            .unwrap();
        }
    }

    #[test]
    fn legacy_uniforms_reject_wrong_types_aliases_arrays_and_runtime_inputs() {
        let fixture = LegacyFixture::new();
        for declaration in [
            "uniform float g_Unknown;",
            "uniform vec2 g_Brightness; // {\"material\":\"Bright\",\"default\":1}",
            "uniform float g_Brightness; // {\"material\":\"Other\",\"default\":1}",
            "uniform float g_Brightness[2]; // {\"material\":\"Bright\",\"default\":1}",
            "uniform sampler2D g_Texture0; // {\"default\":\"_rt_FullFrameBuffer\"}",
            "uniform vec4 g_Color4;",
        ] {
            let fragment = format!("{declaration}\n{LEGACY_GENERIC_FRAGMENT}");
            fixture.write("genericimage", LEGACY_VERTEX, &fragment);
            assert!(
                fixture
                    .load("genericimage", &BTreeMap::new(), &[0])
                    .is_err(),
                "accepted {declaration}"
            );
        }
        fixture.write(
            "genericimage",
            &LEGACY_VERTEX.replace("attribute vec3 a_Position", "attribute vec4 a_Position"),
            LEGACY_GENERIC_FRAGMENT,
        );
        assert!(
            fixture
                .load("genericimage", &BTreeMap::new(), &[0])
                .is_err()
        );
    }

    #[test]
    fn legacy_animation_tracks_active_time_and_nonzero_scroll() {
        let fixture = LegacyFixture::new();
        fixture.write("genericimage", LEGACY_VERTEX, LEGACY_GENERIC_FRAGMENT);
        let shader = fixture
            .load("genericimage", &BTreeMap::new(), &[0])
            .unwrap();
        let mut constants = BTreeMap::from([
            ("g_ScrollX".into(), vec![0.0]),
            ("g_ScrollY".into(), vec![0.0]),
        ]);
        assert!(!legacy_is_animated("genericimage", &shader, &constants));
        constants.insert("g_ScrollY".into(), vec![-0.2]);
        assert!(legacy_is_animated("genericimage", &shader, &constants));
        let mut static_shader = shader;
        static_shader
            .uniforms
            .retain(|uniform| uniform.name != "g_Time");
        assert!(!legacy_is_animated("flag", &static_shader, &constants));
        assert!(!legacy_is_animated(
            "effects/shake",
            &static_shader,
            &constants
        ));
    }

    #[test]
    fn legacy_inert_constants_are_a_bounded_scalar_field_allowlist() {
        for key in ["Alpha", "Power", "Speed", "Scroll 1 Y"] {
            assert!(legacy_constant_is_inert("flowimage", key));
        }
        for key in ["Power", "Scroll 1 X"] {
            assert!(legacy_constant_is_inert("swayimage", key));
        }
        for (name, key) in [
            ("genericimage", "Alpha"),
            ("flag", "Power"),
            ("flowimage", "Speed0"),
            ("swayimage", "UNKNOWN"),
        ] {
            assert!(!legacy_constant_is_inert(name, key));
        }
    }

    #[test]
    fn legacy_entry_renaming_leaves_comments_and_identifier_prefixes_untouched() {
        let source = "/* main a_TexCoord\nmain */\nvoid main(){float mainWeight=0.0;gl_FragColor=vec4(mainWeight);}\n// main";
        let fragment = legacy_fragment(source).unwrap();
        assert!(fragment.contains("/* main a_TexCoord\nmain */"));
        assert!(fragment.contains("float mainWeight=0.0"));
        assert_eq!(fragment.matches("void lwe_LegacyMain()").count(), 1);
    }

    #[test]
    #[ignore = "requires an explicitly supplied original Wallpaper Engine installation; CPU only"]
    fn legacy_original_asset_interfaces() {
        let install = std::path::PathBuf::from(
            std::env::var_os("LWE_LEGACY_TEST_INSTALL").expect("set LWE_LEGACY_TEST_INSTALL"),
        );
        let builtin = install.join("assets");
        for (project, name, slots, combos) in [
            ("deep_space", "genericimage", vec![0], BTreeMap::new()),
            ("deep_space", "flowimage", vec![0, 1, 2, 3], BTreeMap::new()),
            ("beach", "flowimage", vec![0, 1], BTreeMap::new()),
            ("beach", "swayimage", vec![0, 1], BTreeMap::new()),
            (
                "eagleflag",
                "flag",
                vec![0, 1, 2],
                BTreeMap::from([("TINT".into(), 1), ("TEX1FORMAT".into(), 4)]),
            ),
        ] {
            let assets = SceneAssets::open(
                &install.join("projects/defaultprojects").join(project),
                Some(&builtin),
            )
            .unwrap();
            let shader =
                load_legacy_image(&assets, name, &combos, &slots.into_iter().collect()).unwrap();
            assert!(
                shader
                    .uniforms
                    .iter()
                    .any(|uniform| uniform.name == "g_Texture0Resolution")
            );
        }
    }

    #[test]
    fn metadata_provides_material_names_and_combo_defaults() {
        let mut uniforms = BTreeMap::new();
        let mut combos = BTreeMap::new();
        inspect("// [COMBO] {\"combo\":\"SAMPLES\",\"default\":3}\nuniform float g_Strength; // {\"material\":\"strength\",\"default\":0.1}", &mut uniforms, &mut combos).unwrap();
        assert_eq!(combos["SAMPLES"], 3);
        assert_eq!(uniforms["g_Strength"].material.as_deref(), Some("strength"));
    }

    #[test]
    fn material_range_metadata_is_bounded_and_kept_for_constant_validation() {
        let mut uniforms = BTreeMap::new();
        inspect(
            "uniform float strength; // {\"material\":\"strength\",\"range\":[0,2],\"default\":1}\nuniform vec2 repeat; // {\"material\":\"repeat\",\"range\":[0.1,10],\"default\":\"1 1\"}",
            &mut uniforms,
            &mut BTreeMap::new(),
        ).unwrap();
        assert_eq!(uniforms["strength"].material_range, Some([0.0, 2.0]));
        assert_eq!(uniforms["repeat"].material_range, Some([0.1, 10.0]));
        for range in ["[2,0]", "[0]", "[0,1,2]", "[0,1e39]", "[0,\"2\"]", "null"] {
            assert!(
                inspect(
                    &format!(
                        "uniform float strength; // {{\"material\":\"strength\",\"range\":{range}}}"
                    ),
                    &mut BTreeMap::new(),
                    &mut BTreeMap::new()
                )
                .is_err(),
                "accepted {range}"
            );
        }
    }

    #[test]
    fn spectrum_uniforms_require_exact_runtime_type_and_length() {
        for (declaration, accepted) in [
            ("uniform float g_AudioSpectrum16Left[16];", true),
            ("uniform float g_AudioSpectrum16Right[16];", true),
            ("uniform float g_AudioSpectrum16Left;", false),
            ("uniform float g_AudioSpectrum16Left[15];", false),
            ("uniform vec2 g_AudioSpectrum16Left[16];", false),
            ("uniform float g_AudioSpectrum16Left[COUNT];", false),
            ("uniform float Other[16];", false),
            (
                "uniform float g_AudioSpectrum16Left[16]; // {\"material\":\"other\"}",
                false,
            ),
        ] {
            let mut uniforms = BTreeMap::new();
            inspect(declaration, &mut uniforms, &mut BTreeMap::new()).unwrap();
            assert_eq!(
                validate_bounded_uniforms(&uniforms).is_ok(),
                accepted,
                "{declaration}"
            );
        }
        assert!(inspect(
            "uniform float g_AudioSpectrum16Left[16];\nuniform float g_AudioSpectrum16Left[15];",
            &mut BTreeMap::new(),
            &mut BTreeMap::new(),
        ).is_err());
    }

    #[test]
    fn active_shader_preserves_integer_aliases_and_commented_branch_boundaries() {
        let source = "#define FIRST 0\n#define SECOND 7 // named authored option\n#define ALIAS SECOND\n#if SHAPE == ALIAS\nselected\n#else /* inactive */\nwrong\n#endif // author comment\n#undef ALIAS\n#ifdef ALIAS\nwrong-again\n#endif\n";
        let active = active_source(source, &BTreeMap::from([("SHAPE".into(), 7)])).unwrap();
        assert!(active.contains("selected"));
        assert!(!active.contains("wrong"));
        assert!(active_source("#if SHAPE\nx", &BTreeMap::new()).is_err());
        let delayed = "#define A B\n#define B 7\n#if A == 7\nlate-alias\n#endif\n";
        assert!(
            active_source(delayed, &BTreeMap::new())
                .unwrap()
                .contains("late-alias")
        );
        assert!(
            active_source(
                "#define A B\n#define B A\n#if A\nx\n#endif\n",
                &BTreeMap::new()
            )
            .is_err()
        );
        assert!(active_source("#define A (1 + 2)\n#if A\nx\n#endif\n", &BTreeMap::new()).is_err());
    }

    #[test]
    fn bounded_audio_arrays_require_the_exact_stereo_resolution() {
        for resolution in [16, 32, 64] {
            for channel in ["Left", "Right"] {
                let source =
                    format!("uniform float g_AudioSpectrum{resolution}{channel}[{resolution}];");
                let mut uniforms = BTreeMap::new();
                inspect(&source, &mut uniforms, &mut BTreeMap::new()).unwrap();
                assert!(validate_bounded_uniforms(&uniforms).is_ok());
                let malformed = source.replace(&format!("[{resolution}]"), "[17]");
                let mut uniforms = BTreeMap::new();
                inspect(&malformed, &mut uniforms, &mut BTreeMap::new()).unwrap();
                assert!(validate_bounded_uniforms(&uniforms).is_err());
            }
        }
    }

    #[test]
    fn sprite_region_shader_has_one_explicit_flip_and_rejects_unmodeled_branches() {
        let shader = image_material_variant(
            &BTreeMap::from([("version".into(), 2), ("SPRITESHEET".into(), 1)]),
            true,
        )
        .unwrap();
        assert!(
            shader
                .fragment
                .contains("v_UV*g_Texture0UVTransform.xy+g_Texture0UVTransform.zw")
        );
        assert!(!shader.fragment.contains("uv.y+="));
        assert_eq!(
            shader
                .uniforms
                .iter()
                .filter(|u| u.name == "g_Texture0UVTransform")
                .count(),
            1
        );
        assert!(
            image_material_variant(&BTreeMap::from([("SPRITESHEET".into(), 1)]), false).is_err()
        );
        assert!(image_material_variant(&BTreeMap::from([("SKINNING".into(), 1)]), false).is_err());
        assert!(image_material_variant(&BTreeMap::from([("VERSION".into(), 3)]), false).is_err());
    }

    #[test]
    fn bounded_project_pass_retains_project_priority_and_rejects_unknown_interfaces() {
        let project = LegacyFixture::new();
        let builtin = LegacyFixture::new();
        let name = "workshop/123/effects/ownshader";
        project.write(
            name,
            LEGACY_VERTEX,
            &format!("{LEGACY_GENERIC_FRAGMENT}// project shader marker\n"),
        );
        builtin.write(
            name,
            LEGACY_VERTEX,
            &format!("{LEGACY_GENERIC_FRAGMENT}// builtin shader marker\n"),
        );
        let assets = SceneAssets::open(&project.0, Some(&builtin.0)).unwrap();
        let shader = load(&assets, name, &BTreeMap::new(), &BTreeSet::from([0])).unwrap();
        assert!(shader.fragment.contains("project shader marker"));
        assert!(!shader.fragment.contains("builtin shader marker"));
        for uniform in [
            "uniform float g_Unknown;",
            "uniform mat4 custom; // {\"material\":\"matrix\",\"default\":1}",
            "uniform float values[16]; // {\"material\":\"values\",\"default\":1}",
            "uniform sampler2D g_Texture8;",
        ] {
            let fragment = format!(
                "{uniform}\n{LEGACY_GENERIC_FRAGMENT}\nvoid useExtra(){{float extra=0.0;extra=extra;}}\n"
            );
            project.write(name, LEGACY_VERTEX, &fragment);
            // Unknown active runtime/material declarations fail closed. A
            // sampler is checked only when referenced by the selected branch.
            let fragment = if uniform.contains("g_Texture8") {
                fragment.replace("texSample2D(g_Texture0", "texSample2D(g_Texture8")
            } else {
                fragment
            };
            project.write(name, LEGACY_VERTEX, &fragment);
            assert!(
                load(
                    &SceneAssets::open(&project.0, Some(&builtin.0)).unwrap(),
                    name,
                    &BTreeMap::new(),
                    &BTreeSet::from([0])
                )
                .is_err(),
                "accepted {uniform}"
            );
        }
    }

    #[test]
    fn image2_card_removes_inert_uniforms_and_does_not_duplicate_tint() {
        let fixture = LegacyFixture::new();
        let vertex = format!("uniform vec4 g_Texture0Rotation;\n{LEGACY_VERTEX}");
        let fragment = "uniform vec4 g_Color4;\nuniform sampler2D g_Texture0;\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord)*g_Color4;}\n";
        fixture.write("genericimage2", &vertex, fragment);
        let shader = load_card_image(
            &SceneAssets::open(&fixture.0, None).unwrap(),
            "genericimage2",
            &BTreeMap::from([("VERSION".into(), 2)]),
            &BTreeSet::from([0]),
        )
        .unwrap();
        assert!(
            !shader
                .uniforms
                .iter()
                .any(|uniform| uniform.name == "g_Texture0Rotation")
        );
        assert_eq!(shader.fragment.matches("*g_Color4").count(), 1);
        assert!(!shader.fragment.contains("lwe_LegacyMain"));
        assert!(shader.vertex.contains("lwe_LegacyMain"));
    }

    #[test]
    fn image2_material_combo_case_selects_the_same_branch_without_alias_collisions() {
        let fixture = LegacyFixture::new();
        let fragment = "uniform vec4 g_Color4;\nuniform sampler2D g_Texture0;\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord)*g_Color4;}\n";
        fixture.write("genericimage2", LEGACY_VERTEX, fragment);
        let assets = SceneAssets::open(&fixture.0, None).unwrap();
        let load =
            |values| load_card_image(&assets, "genericimage2", &values, &BTreeSet::from([0]));
        let canonical = load(BTreeMap::from([("VERSION".into(), 2)])).unwrap();
        let authored = load(BTreeMap::from([("version".into(), 2)])).unwrap();
        assert_eq!(authored.vertex, canonical.vertex);
        assert_eq!(authored.fragment, canonical.fragment);
        for alternate in [1, 2] {
            assert!(
                load(BTreeMap::from([
                    ("VERSION".into(), 2),
                    ("version".into(), alternate),
                ]))
                .unwrap_err()
                .to_string()
                .contains("ambiguous")
            );
        }
        assert!(load(BTreeMap::from([("unknown".into(), 0)])).is_err());
        assert!(load(BTreeMap::from([("lighting".into(), 1)])).is_err());
    }

    #[test]
    fn image2_missing_version_preserves_material_color_and_card_geometry() {
        let fixture = LegacyFixture::new();
        let fragment = r#"uniform sampler2D g_Texture0;
varying vec2 v_TexCoord;
#ifndef VERSION
uniform float g_Brightness; // {"material":"Brightness","default":1,"range":[0,2]}
uniform float g_UserAlpha; // {"material":"Alpha","default":1,"range":[0,1]}
#else
uniform vec4 g_Color4;
#endif
void main(){
vec4 sampleColor=texSample2D(g_Texture0,v_TexCoord);
#ifndef VERSION
sampleColor.rgb=sampleColor.rgb*g_Brightness;
sampleColor.a=sampleColor.a*g_UserAlpha;
#else
sampleColor=sampleColor*g_Color4;
#endif
gl_FragColor=sampleColor;
}
"#;
        fixture.write("genericimage2", LEGACY_VERTEX, fragment);
        let assets = SceneAssets::open(&fixture.0, None).unwrap();
        let legacy = load_card_image(
            &assets,
            "genericimage2",
            &BTreeMap::new(),
            &BTreeSet::from([0]),
        )
        .unwrap();
        assert!(legacy.vertex.starts_with("#version 130\n"));
        assert!(legacy.vertex.contains("#define mul(a,b) ((b)*(a))"));
        assert!(legacy.vertex.contains("in vec3 a_Position;"));
        assert!(legacy.vertex.contains("in vec2 a_TexCoord;"));
        assert!(legacy.vertex.contains(
            "vec2(a_TexCoord.x,1.0-a_TexCoord.y)*g_Texture0Resolution.zw/g_Texture0Resolution.xy"
        ));
        assert!(
            legacy
                .fragment
                .contains("sampleColor.rgb=sampleColor.rgb*g_Brightness")
        );
        assert!(
            legacy
                .fragment
                .contains("sampleColor.a=sampleColor.a*g_UserAlpha")
        );
        assert!(legacy.fragment.contains("uniform vec4 g_Color4;"));
        assert_eq!(legacy.fragment.matches("*=g_Color4").count(), 1);
        assert!(
            legacy
                .fragment
                .contains("void main(){lwe_LegacyMain();lwe_FragColor*=g_Color4;}")
        );
        assert!(!legacy.fragment.contains("#define VERSION"));
        for (name, material, range) in [
            ("g_Brightness", "Brightness", [0.0, 2.0]),
            ("g_UserAlpha", "Alpha", [0.0, 1.0]),
        ] {
            let uniform = legacy.uniforms.iter().find(|u| u.name == name).unwrap();
            assert_eq!(uniform.material.as_deref(), Some(material));
            assert_eq!(uniform.default, Some(Value::from(1)));
            assert_eq!(uniform.material_range, Some(range));
        }
        for version in [1, 2] {
            let tinted = load_card_image(
                &assets,
                "genericimage2",
                &BTreeMap::from([("VERSION".into(), version)]),
                &BTreeSet::from([0]),
            )
            .unwrap();
            assert!(tinted.fragment.contains("sampleColor=sampleColor*g_Color4"));
            assert!(!tinted.fragment.contains("g_Brightness"));
            assert!(!tinted.fragment.contains("g_UserAlpha"));
            assert!(!tinted.fragment.contains("lwe_LegacyMain"));
            assert_eq!(tinted.fragment.matches("*g_Color4").count(), 1);
            assert_eq!(
                tinted
                    .vertex
                    .replace(&format!("#define VERSION {version}\n"), ""),
                legacy.vertex
            );
        }
    }

    #[test]
    fn image2_legacy_version_keeps_unknown_inputs_and_wrong_interfaces_blocked() {
        for values in [
            BTreeMap::from([("VERSION".into(), 0)]),
            BTreeMap::from([("VERSION".into(), 3)]),
            BTreeMap::from([("LIGHTING".into(), 1)]),
            BTreeMap::from([("SKINNING".into(), 1)]),
            BTreeMap::from([("SPRITESHEET".into(), 1)]),
            BTreeMap::from([("UNKNOWN".into(), 0)]),
        ] {
            assert!(image2_combos(&values, false).is_err());
        }
        assert!(image2_combos(&BTreeMap::new(), true).is_err());
        assert!(image2_combos(&BTreeMap::from([("VERSION".into(), 1)]), true).is_err());
        let fixture = LegacyFixture::new();
        let fragment = "uniform sampler2D g_Texture0;\nuniform vec4 g_Color4;\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord)*g_Color4;}\n";
        fixture.write("genericimage2", LEGACY_VERTEX, fragment);
        assert!(
            load_card_image(
                &SceneAssets::open(&fixture.0, None).unwrap(),
                "genericimage2",
                &BTreeMap::new(),
                &BTreeSet::from([0]),
            )
            .is_err()
        );
        for declaration in [
            "uniform vec2 g_Brightness; // {\"material\":\"Brightness\",\"default\":1}",
            "uniform float g_Brightness; // {\"material\":\"unvalidated\",\"default\":1}",
            "uniform float g_Brightness;",
        ] {
            fixture.write(
                "genericimage2",
                LEGACY_VERTEX,
                &format!("{declaration}\nuniform float g_UserAlpha; // {{\"material\":\"Alpha\",\"default\":1}}\nuniform sampler2D g_Texture0;\nvarying vec2 v_TexCoord;\nvoid main(){{gl_FragColor=texSample2D(g_Texture0,v_TexCoord)*vec4(vec3(g_Brightness),g_UserAlpha);}}\n"),
            );
            assert!(
                load_card_image(
                    &SceneAssets::open(&fixture.0, None).unwrap(),
                    "genericimage2",
                    &BTreeMap::new(),
                    &BTreeSet::from([0]),
                )
                .is_err(),
                "accepted {declaration}"
            );
        }
    }

    #[test]
    fn shared_dialect_makes_initializer_casts_explicit_without_changing_formulas() {
        let source = "// float comment = untouched;\nvoid main(){vec3 color = vec4(0.1,0.2,0.3,0.4); float first = vec2(0.2,0.7) * scale; for(int index = -amount * 10; index <= amount * 10; ++index){} const vec4 captured = texSample2D(g_Texture0, vec4(uv,0,0)); color=pow(color,exponent);}";
        let result = adapt_shared_dialect(source, false, &BTreeMap::new()).unwrap();
        assert!(result.contains("// float comment = untouched;"));
        assert!(result.contains("vec3 color = vec3(vec4(0.1,0.2,0.3,0.4))"));
        assert!(result.contains("float first = float(vec2(0.2,0.7) * scale)"));
        assert!(result.contains("int index = int(-amount * 10)"));
        assert!(
            result.contains("vec4 captured = vec4(lwe_SharedTexture(g_Texture0, vec4(uv,0,0)))")
        );
        assert!(!result.contains("const vec4 captured"));
        assert!(result.contains("color=lwe_SharedPow(color,exponent)"));
        assert!(result.contains("texture(image,uv.xy)"));
    }

    #[test]
    fn bounded_pass_combos_do_not_enable_geometry_or_unknown_inputs() {
        for (name, combo, value) in [
            ("effects/scroll", "COPYBG", 0),
            ("effects/pulse", "AUDIOPROCESSING", 4),
            ("workshop/123/ownshader", "UNKNOWN", 0),
            ("workshop/123/ownshader", "LIGHTING", 1),
            ("workshop/123/ownshader", "BLENDMODE", 32),
        ] {
            assert!(
                validate_effect_combos(name, &BTreeMap::from([(combo.to_owned(), value)])).is_err()
            );
        }
        assert!(
            image2_combos(
                &BTreeMap::from([("VERSION".into(), 2), ("BLENDMODE".into(), 31)]),
                false
            )
            .is_err()
        );
        for mode in [22, 28, 30, 31] {
            let combos = BTreeMap::from([("VERSION".into(), 2), ("BLENDMODE".into(), mode)]);
            assert!(image2_combos(&combos, false).is_err());
            image2_combos(&combos, true).unwrap();
        }
        assert!(
            image2_combos(
                &BTreeMap::from([("VERSION".into(), 2), ("BLENDMODE".into(), 9)]),
                true
            )
            .is_err()
        );
    }

    #[test]
    #[ignore = "requires explicitly supplied user-owned scene and builtin shaders; CPU only"]
    fn original_image2_card_versions_prepare() {
        let project = std::path::PathBuf::from(
            std::env::var_os("LWE_PROJECT_SHADER_TEST_PROJECT")
                .expect("set LWE_PROJECT_SHADER_TEST_PROJECT"),
        );
        let builtin = std::path::PathBuf::from(
            std::env::var_os("LWE_PROJECT_SHADER_TEST_BUILTIN")
                .expect("set LWE_PROJECT_SHADER_TEST_BUILTIN"),
        );
        let assets = SceneAssets::open(&project, Some(&builtin)).unwrap();
        let mut shaders = Vec::new();
        for (name, values) in [
            ("card-legacy", BTreeMap::new()),
            ("card-version1", BTreeMap::from([("VERSION".into(), 1)])),
            ("card-version2", BTreeMap::from([("VERSION".into(), 2)])),
            (
                "card-lowercase-version2",
                BTreeMap::from([("version".into(), 2)]),
            ),
        ] {
            let shader =
                load_card_image(&assets, "genericimage2", &values, &BTreeSet::from([0])).unwrap();
            let names = shader
                .uniforms
                .iter()
                .map(|uniform| uniform.name.as_str())
                .collect::<BTreeSet<_>>();
            if values.is_empty() {
                assert!(names.contains("g_Brightness") && names.contains("g_UserAlpha"));
            } else {
                assert!(!names.contains("g_Brightness") && !names.contains("g_UserAlpha"));
            }
            assert!(names.contains("g_Color4"));
            shaders.push((name.to_owned(), shader));
        }
        for mode in [0, 22, 28, 30, 31] {
            shaders.push((
                format!("composite-{mode}"),
                load_image_composite(&assets, mode).unwrap(),
            ));
        }
        if let Some(output) = std::env::var_os("LWE_PROJECT_SHADER_TEST_OUTPUT") {
            let output = std::path::PathBuf::from(output);
            std::fs::create_dir_all(&output).unwrap();
            for (name, shader) in shaders {
                std::fs::write(output.join(format!("{name}.vert")), shader.vertex).unwrap();
                std::fs::write(output.join(format!("{name}.frag")), shader.fragment).unwrap();
            }
        }
    }

    #[test]
    #[ignore = "requires explicitly supplied user-owned scene and builtin shaders; CPU only"]
    fn original_bounded_project_shader_interfaces() {
        let project = std::path::PathBuf::from(
            std::env::var_os("LWE_PROJECT_SHADER_TEST_PROJECT")
                .expect("set LWE_PROJECT_SHADER_TEST_PROJECT"),
        );
        let builtin = std::path::PathBuf::from(
            std::env::var_os("LWE_PROJECT_SHADER_TEST_BUILTIN")
                .expect("set LWE_PROJECT_SHADER_TEST_BUILTIN"),
        );
        let assets = SceneAssets::open(&project, Some(&builtin)).unwrap();
        let mut shaders = vec![(
            "card".to_owned(),
            load_card_image(
                &assets,
                "genericimage2",
                &BTreeMap::from([("VERSION".into(), 2)]),
                &BTreeSet::from([0]),
            )
            .unwrap(),
        )];
        for (name, values) in [
            ("card-legacy", BTreeMap::new()),
            ("card-version1", BTreeMap::from([("VERSION".into(), 1)])),
        ] {
            shaders.push((
                name.to_owned(),
                load_card_image(&assets, "genericimage2", &values, &BTreeSet::from([0])).unwrap(),
            ));
        }
        for mode in [0, 22, 28, 30, 31] {
            shaders.push((
                format!("composite-{mode}"),
                load_image_composite(&assets, mode).unwrap(),
            ));
        }
        for (name, combos, slots) in [
            ("passthrough", BTreeMap::new(), vec![0]),
            ("effects/scroll", BTreeMap::new(), vec![0]),
            (
                "effects/pulse",
                BTreeMap::from([("AUDIOPROCESSING".into(), 3)]),
                vec![0],
            ),
            (
                "workshop/2423877731/effects/chromatic_aberration",
                BTreeMap::from([("AUDIOPROCESSING".into(), 3)]),
                vec![0],
            ),
            (
                "workshop/2822917890/effects/light_map",
                BTreeMap::new(),
                vec![0, 1],
            ),
            (
                "workshop/2822917890/effects/blur_gaussian",
                BTreeMap::new(),
                vec![0],
            ),
            (
                "workshop/2822917890/effects/blur_gaussian",
                BTreeMap::from([("VERTICAL".into(), 1)]),
                vec![0],
            ),
            (
                "workshop/2822917890/effects/apply",
                BTreeMap::new(),
                vec![0, 2],
            ),
        ] {
            let shader = load(&assets, name, &combos, &slots.into_iter().collect()).unwrap();
            if name == "effects/pulse" || name.ends_with("chromatic_aberration") {
                assert_eq!(
                    shader
                        .uniforms
                        .iter()
                        .filter(|uniform| is_audio_spectrum(uniform))
                        .count(),
                    2
                );
            }
            shaders.push((
                format!("{}-{}", name.replace('/', "_"), shaders.len()),
                shader,
            ));
        }
        if let Some(output) = std::env::var_os("LWE_PROJECT_SHADER_TEST_OUTPUT") {
            let output = std::path::PathBuf::from(output);
            std::fs::create_dir_all(&output).unwrap();
            for (name, shader) in shaders {
                std::fs::write(output.join(format!("{name}.vert")), shader.vertex).unwrap();
                std::fs::write(output.join(format!("{name}.frag")), shader.fragment).unwrap();
            }
        }
    }

    #[test]
    fn disabled_editor_combo_still_declares_the_runtime_default() {
        let mut uniforms = BTreeMap::new();
        let mut combos = BTreeMap::new();
        inspect(
            "// [COMBO_OFF] {\"combo\":\"SPECULAR\",\"default\":0}",
            &mut uniforms,
            &mut combos,
        )
        .unwrap();
        assert_eq!(combos["SPECULAR"], 0);
    }

    #[test]
    fn particle_combos_do_not_enable_unimplemented_vertex_or_engine_features() {
        for name in [
            "LIGHTING",
            "REFRACT",
            "FOG_DIST",
            "FOG_HEIGHT",
            "ALPHATOCOVERAGE",
            "TEX0FORMAT",
            "UNKNOWN",
        ] {
            assert!(validate_particle_combos(&BTreeMap::from([(name.into(), 1)])).is_err());
        }
        validate_particle_combos(&BTreeMap::from([
            ("REFRACT".into(), 0),
            ("CUTOUT".into(), 1),
            ("FOG".into(), 1),
            ("SPRITESHEET".into(), 1),
            ("SPRITESHEETBLEND".into(), 1),
            ("TEX0FORMAT".into(), 9),
        ]))
        .unwrap();
        let selected = active_source(
            "#if LIGHTING\n#include \"common_pbr_2.h\"\n#require LightingV1\n#endif\nvec4 color;",
            &BTreeMap::from([("LIGHTING".into(), 0)]),
        )
        .unwrap();
        assert!(!selected.contains("LightingV1"));
        assert!(!selected.contains("common_pbr_2.h"));
        assert!(selected.contains("vec4 color;"));
    }

    #[test]
    fn new_effect_combos_reject_unvalidated_geometry_and_macro_values() {
        for (name, combo, value) in [
            ("effects/waterripple", "PERSPECTIVE", 1),
            ("effects/waterripple", "MASK", 2),
            ("effects/waterripple", "UNVALIDATED", 0),
            ("effects/iris", "BACKGROUND", -1),
            ("effects/iris", "PERSPECTIVE", 0),
        ] {
            assert!(
                validate_effect_combos(name, &BTreeMap::from([(combo.to_owned(), value)])).is_err(),
                "unvalidated combo {name}/{combo}={value} was accepted"
            );
        }
        validate_effect_combos(
            "effects/waterripple",
            &BTreeMap::from([
                ("PERSPECTIVE".to_owned(), 0),
                ("MASK".to_owned(), 1),
                ("SPECULAR".to_owned(), 1),
            ]),
        )
        .unwrap();
        validate_effect_combos(
            "effects/iris",
            &BTreeMap::from([("MASK".to_owned(), 1), ("BACKGROUND".to_owned(), 1)]),
        )
        .unwrap();
    }

    #[test]
    fn adaptation_preserves_bloom_varying_arrays_and_texel_uniforms() {
        let source = "attribute vec2 a_TexCoord;\nuniform vec2 g_TexelSize;\nvarying vec2 v_Coordinates[13];\nvoid main(){v_Coordinates[0]=a_TexCoord-g_TexelSize;}";
        let adapted = adapt(source, true, &BTreeMap::new()).unwrap();
        assert!(adapted.contains("out vec2 v_Coordinates[13]"));
        assert!(adapted.contains("uniform vec2 g_TexelSize;"));
        assert!(adapted.contains("v_Coordinates[0]=a_TexCoord-g_TexelSize"));
    }

    #[test]
    fn adaptation_hoists_sampler_and_preserves_identifier_boundaries() {
        let adapted = adapt("varying vec2 coord;\nvec4 f(){return texSample2D(g_Texture0,coord);}\nuniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=f();}\nfloat attributeWeight;", false, &BTreeMap::new()).unwrap();
        assert!(adapted.find("uniform sampler2D").unwrap() < adapted.find("vec4 f()").unwrap());
        assert!(adapted.contains("in vec2 coord"));
        assert!(adapted.contains("lwe_FragColor=f()"));
        assert!(adapted.contains("attributeWeight"));
    }

    #[test]
    fn platform_macro_override_is_rejected() {
        assert!(adapt("", true, &BTreeMap::from([("HLSL".into(), 1)])).is_err());
    }

    #[test]
    fn inactive_sampler_defaults_are_not_loaded_and_else_is_selected() {
        let source = "#if COPYBG\nuniform sampler2D g_Texture2; // {\"default\":\"_rt_FullFrameBuffer\"}\n#elif NOISE == 1\nuniform sampler2D g_Texture3;\n#else\nuniform sampler2D g_Texture0;\n#endif";
        let active = active_source(
            source,
            &BTreeMap::from([("COPYBG".into(), 0), ("NOISE".into(), 0)]),
        )
        .unwrap();
        assert!(active.contains("g_Texture0"));
        assert!(!active.contains("g_Texture2"));
        assert!(!active.contains("g_Texture3"));
        assert!(
            condition(
                "NOISE == 1 || (COPYBG == 0 && !NOISE)",
                &BTreeMap::from([("NOISE".into(), 0), ("COPYBG".into(), 0)])
            )
            .unwrap()
        );
    }

    #[test]
    fn unconditional_mask_sampler_declarations_are_not_active_uses() {
        for (name, index) in [("g_Texture3", 3), ("g_Texture1", 1)] {
            let source = format!(
                "uniform sampler2D {name}; // {{\"combo\":\"MASK\"}}\n#if MASK\nvec4 mask=texture({name},vec2(0));\n#endif\n"
            );
            let inactive = active_source(&source, &BTreeMap::from([("MASK".into(), 0)])).unwrap();
            assert!(
                !referenced_sampler(&inactive, name),
                "inactive texture slot {index} was treated as required"
            );
            let active = active_source(&source, &BTreeMap::from([("MASK".into(), 1)])).unwrap();
            assert!(
                referenced_sampler(&active, name),
                "enabled mask texture slot {index} was lost"
            );
        }
    }
}
