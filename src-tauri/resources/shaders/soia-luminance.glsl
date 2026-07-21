// Soia internal luminance adjustment.
//
// The adjustment is applied in scene-linear light before scaling and output
// color management. A scale of 1.0 is an exact no-op.

//!PARAM soia_luminance_scale
//!DESC Scene-linear luminance multiplier
//!TYPE DYNAMIC float
//!MINIMUM 0.125
//!MAXIMUM 8.0
1.0
//!HOOK MAIN
//!BIND HOOKED
//!DESC Soia luminance adjustment

vec4 hook() {
    vec4 color = HOOKED_texOff(0);
    if (abs(soia_luminance_scale - 1.0) < 0.000001) {
        return color;
    }

    vec4 linear = linearize(color);
    linear.rgb *= soia_luminance_scale;
    return delinearize(linear);
}
