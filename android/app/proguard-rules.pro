# JNI entry points are looked up by name from Rust; keep them.
-keepclasseswithmembernames class * {
    native <methods>;
}

# Rust calls ProgressCallback.onProgress(int,long,long,long) via JNI
# call_method by name; R8 must not rename it or the call fails at runtime.
-keepclassmembers class * {
    void onProgress(int,long,long,long);
}