use std::env;
use webrender_build::shader::*;
use webrender_build::shader_features::{ShaderFeatureFlags, get_shader_features};

use std::borrow::Cow;
use std::fs::{canonicalize, read_dir, File};
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;

#[derive(Clone, Debug)]
struct ShaderOptimizationInput {
    shader_name: &'static str,
    config: String,
    gl_version: ShaderVersion,
}

#[derive(Debug)]
struct ShaderOptimizationOutput {
    full_shader_name: String,
    gl_version: ShaderVersion,
    vert_file_path: PathBuf,
    frag_file_path: PathBuf,
    digest: ProgramSourceDigest,
}

#[derive(Debug)]
struct ShaderOptimizationError {
    shader: ShaderOptimizationInput,
    message: String,
}

fn main() {
    let shader_versions = [ShaderVersion::Gl];

    let mut shaders = Vec::default();
    for &gl_version in &shader_versions {
        let mut flags = ShaderFeatureFlags::all();
        if gl_version != ShaderVersion::Gl {
            flags.remove(ShaderFeatureFlags::GL);
        }
        if gl_version != ShaderVersion::Gles {
            flags.remove(ShaderFeatureFlags::GLES);
            flags.remove(ShaderFeatureFlags::TEXTURE_EXTERNAL);
        }
        flags.remove(ShaderFeatureFlags::DITHERING);
        flags.remove(ShaderFeatureFlags::PIXEL_LOCAL_STORAGE);

        for (shader_name, configs) in get_shader_features(flags) {
            for config in configs {
                shaders.push(ShaderOptimizationInput {
                    shader_name,
                    config,
                    gl_version,
                });
            }
        }
    }

    // println!("shaders:\n{:#?}", shaders);

    let shader_dir = Path::new("/home/jamie/src/gecko/gfx/wr/webrender/res");
    let out_dir = env::var("OUT_DIR").unwrap_or("out".to_owned());

    let outputs = build_parallel::compile_objects(&|shader: &ShaderOptimizationInput| {
        // println!("Optimizing shader {:?}", shader);
        let target = match shader.gl_version {
            ShaderVersion::Gl => glslopt::Target::OpenGl,
            ShaderVersion::Gles => glslopt::Target::OpenGles30,
        };
        let glslopt_ctx = glslopt::Context::new(target);

        let features = shader.config.split(",").filter(|f| !f.is_empty()).collect::<Vec<_>>();

        let (vert_src, frag_src) = build_shader_strings(
            shader.gl_version,
            &features,
            shader.shader_name,
            &|f| Cow::Owned(shader_source_from_file(&shader_dir.join(&format!("{}.glsl", f)))),
        );

        let full_shader_name = if shader.config.is_empty() {
            shader.shader_name.to_string()
        } else {
            format!("{}_{}", shader.shader_name, shader.config.replace(",", "_"))
        };

        let vert = glslopt_ctx.optimize(glslopt::ShaderType::Vertex, vert_src);
        if !vert.get_status() {
            return Err(ShaderOptimizationError {
                shader: shader.clone(),
                message: vert.get_log().to_string(),
            });
        }
        let frag = glslopt_ctx.optimize(glslopt::ShaderType::Fragment, frag_src);
        if !frag.get_status() {
            return Err(ShaderOptimizationError {
                shader: shader.clone(),
                message: frag.get_log().to_string(),
            });
        }

        let vert_source = vert.get_output().unwrap();
        let frag_source = frag.get_output().unwrap();

        // Compute a digest of the optimized shader sources. We store this
        // as a literal alongside the source string so that we don't need
        // to hash large strings at runtime.
        let mut hasher = DefaultHasher::new();
        hasher.write(vert_source.as_bytes());
        hasher.write(frag_source.as_bytes());
        let digest: ProgramSourceDigest = hasher.into();

        let vert_file_path = Path::new(&out_dir)
            .join(format!("{}_{:?}.vert", full_shader_name, shader.gl_version));
        let mut vert_file = File::create(&vert_file_path).unwrap();
        vert_file.write_all(vert_source.as_bytes()).unwrap();
        let frag_file_path = vert_file_path.with_extension("frag");
        let mut frag_file = File::create(&frag_file_path).unwrap();
        frag_file.write_all(frag_source.as_bytes()).unwrap();

        // println!("Finished optimizing shader {:?}", shader);

        Ok(ShaderOptimizationOutput {
            full_shader_name,
            gl_version: shader.gl_version,
            vert_file_path,
            frag_file_path,
            digest,
        })
    }, &shaders);

}
