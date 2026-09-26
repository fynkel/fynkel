// The material Inspector's Shader dropdown (ADR-0031 § 7): what a material
// document becomes when it picks another shader. As in Unity, a value
// whose name the new shader also has is kept — `base_color` and
// `baseColor` are the same name, so the built-in Lit's fields carry over
// to a shader's properties and back — when it still fits the type; the
// rest go. The alpha mode moves to one the new shader allows.
import 'package:fynkel_authoring/fynkel_authoring.dart';

import 'asset_edits.dart';

/// The built-in Lit's values as a material document holds them, with the
/// number of components each has.
const pbrValueFields = {
  'base_color': 4,
  'metallic': 1,
  'roughness': 1,
  'emissive': 3,
  'emissive_strength': 1,
  'normal_scale': 1,
  'occlusion_strength': 1,
};

/// The built-in Lit's texture fields (`<slot>_texture`).
const pbrTextureFields = [
  'base_color_texture',
  'metallic_roughness_texture',
  'normal_texture',
  'occlusion_texture',
  'emissive_texture',
];

/// `base_color`, `baseColor` and `BaseColor` are one name.
String sameName(String name) => name.replaceAll('_', '').toLowerCase();

/// What a material document's `shader` field names: `fynkel.pbr@1`, or a
/// shader asset's id (`{ "asset": id }`); null when it names neither.
String? shaderOf(Map<String, Object?> fields) {
  final s = fields['shader'];
  if (s is String) return s;
  if (s is Map && s['asset'] is String) return s['asset'] as String;
  return null;
}

/// [value] as [components] numbers, or null when it does not fit: a
/// colour gains or loses its alpha (RGB ↔ RGBA); a number stays a number.
Object? fitValue(Object? value, int components, {String type = ''}) {
  if (value == null) return null;
  if (type == 'bool') return value is bool ? value : (value is num ? value != 0 : null);
  if (components == 1) {
    if (value is bool) return value ? 1 : 0;
    if (value is num) return type == 'int' ? value.round() : value;
    return null;
  }
  if (value is! List || value.any((v) => v is! num)) return null;
  final v = [for (final x in value) x as num];
  if (v.length == components) return v;
  if (v.length == 4 && components == 3) return v.sublist(0, 3);
  if (v.length == 3 && components == 4) return [...v, 1];
  return null;
}

/// The one edit (JSON pointer → value, or [unset]) that turns the material
/// [fields] into a material of [to]: the shader named, the values it
/// shares with the old one kept, the old shader's own fields removed, the
/// alpha mode one [to] allows.
Map<String, Object?> switchShader(Map<String, Object?> fields, ShaderInfo to) {
  // The old values and textures by their shared name.
  final values = <String, Object?>{};
  final textures = <String, Map<String, Object?>>{};
  final fromPbr = shaderOf(fields) == pbrShader;
  if (fromPbr) {
    for (final f in pbrValueFields.keys) {
      if (fields.containsKey(f)) values[sameName(f)] = fields[f];
    }
    for (final f in pbrTextureFields) {
      final ref = fields[f];
      if (ref is Map) textures[sameName(f.substring(0, f.length - '_texture'.length))] = {'texture': ref};
    }
  } else {
    final params = fields['parameters'];
    if (params is Map) {
      for (final e in params.entries) {
        values[sameName('${e.key}')] = e.value;
      }
    }
    final bound = fields['textures'];
    if (bound is Map) {
      for (final e in bound.entries) {
        if (e.value is Map) textures[sameName('${e.key}')] = Map<String, Object?>.from(e.value as Map);
      }
    }
  }

  final out = <String, Object?>{};
  if (to.builtin) {
    out['/shader'] = pbrShader;
    if (!fromPbr) {
      if (fields.containsKey('parameters')) out['/parameters'] = unset;
      if (fields.containsKey('textures')) out['/textures'] = unset;
    }
    for (final f in pbrValueFields.entries) {
      final v = fitValue(values[sameName(f.key)], f.value);
      if (v != null) out['/${f.key}'] = v;
    }
    for (final f in pbrTextureFields) {
      final binding = textures[sameName(f.substring(0, f.length - '_texture'.length))];
      if (binding?['texture'] is Map) out['/$f'] = binding!['texture'];
    }
  } else {
    out['/shader'] = {'asset': to.shader};
    if (fromPbr) {
      for (final f in [...pbrValueFields.keys, ...pbrTextureFields]) {
        if (fields.containsKey(f)) out['/$f'] = unset;
      }
    }
    final params = <String, Object?>{};
    final bound = <String, Object?>{};
    for (final p in to.properties) {
      if (p.isTexture) {
        final binding = textures[sameName(p.name)];
        if (binding?['texture'] is Map) bound[p.name] = binding;
      } else {
        final v = fitValue(values[sameName(p.name)], p.components, type: p.type);
        if (v != null) params[p.name] = v;
      }
    }
    if (params.isNotEmpty) {
      out['/parameters'] = params;
    } else if (fields.containsKey('parameters')) {
      out['/parameters'] = unset;
    }
    if (bound.isNotEmpty) {
      out['/textures'] = bound;
    } else if (fields.containsKey('textures')) {
      out['/textures'] = unset;
    }
  }

  final allowed = to.builtin ? const ['opaque', 'mask', 'blend'] : to.alpha;
  final mode = fields['alpha_mode'] is String ? fields['alpha_mode'] as String : 'opaque';
  if (allowed.isNotEmpty && !allowed.contains(mode)) out['/alpha_mode'] = allowed.first;
  return out;
}
