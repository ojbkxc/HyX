# JNI entry points are looked up by name from Rust; keep them.
-keepclasseswithmembernames class * {
    native <methods>;
}

# Rust calls ProgressCallback.onProgress(int,long,long,long) via JNI
# call_method by name; R8 must not rename it or the call fails at runtime.
-keepclassmembers class * {
    void onProgress(int,long,long,long);
}

# Rust calls ProgressCallback.onPeerFingerprint(String) via JNI call_method
# by name (TOFU 回传指纹); R8 must not rename it or the call fails at runtime.
-keepclassmembers class * {
    void onPeerFingerprint(java.lang.String);
}

# Rust calls LogCallback.onLog(int,String,String) via JNI call_method by name;
# R8 must not rename it or the log callback fails at runtime.
-keepclassmembers class * {
    void onLog(int,java.lang.String,java.lang.String);
}
