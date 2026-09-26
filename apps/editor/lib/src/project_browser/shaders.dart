// Create ▸ Shader ▸ Lit / Unlit (ADR-0031 § 7): one `.shader` file from a
// template, as Unity's Create ▸ Shader. The header names the shader in the
// material's Shader dropdown (`Custom/New Lit Shader`) and lists its
// properties; the WGSL below it says what the surface is.

/// The template of a new shader named [name] in the dropdown's Custom menu.
String shaderSource({required bool lit, required String name}) => lit
    ? '''
// A lit shader: surface() says what the surface is, the engine lights it.
// The properties are the material's fields in the Inspector.
shader "Custom/$name" {
    lighting lit
    properties {
        base_color color   = (1, 1, 1, 1)          "Multiplies the base map"
        base_map   texture = white
        metallic   float   = 0   range(0, 1)
        roughness  float   = 0.5 range(0, 1)
    }
}

fn surface(in: SurfaceInput) -> Surface {
    var s = default_surface();
    let c = material.base_color * sample_base_map(in.uv0);
    s.base_color = c.rgb;
    s.alpha = c.a;
    s.metallic = material.metallic;
    s.roughness = material.roughness;
    return s;
}
'''
    : '''
// An unlit shader: the colour surface() gives is the colour drawn.
// The properties are the material's fields in the Inspector.
shader "Custom/$name" {
    lighting unlit
    properties {
        base_color color   = (1, 1, 1, 1)          "Multiplies the base map"
        base_map   texture = white
    }
}

fn surface(in: SurfaceInput) -> Surface {
    var s = default_surface();
    let c = material.base_color * sample_base_map(in.uv0);
    s.base_color = c.rgb;
    s.alpha = c.a;
    return s;
}
''';
