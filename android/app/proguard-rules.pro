# JNI entry points are looked up by name from Rust; keep them.
-keepclasseswithmembernames class * {
    native <methods>;
}