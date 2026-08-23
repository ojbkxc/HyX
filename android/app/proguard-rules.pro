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

# ViewModel 子类通过反射创建（viewModels() 委托），keep 构造函数。
-keep class * extends androidx.lifecycle.ViewModel { <init>(...); }

# Application/Activity 在 manifest 注册，但保险起见显式 keep。
-keep class com.ojbkxc.hyx.HyXApp { *; }
-keep class com.ojbkxc.hyx.MainActivity { *; }

# JNI 回调接口 — keep 接口本身和默认实现，防止 R8 移除 DefaultImpls。
-keep interface com.ojbkxc.hyx.core.HyXNative$ProgressCallback { *; }
-keep class com.ojbkxc.hyx.core.HyXNative$ProgressCallback { *; }
-keep interface com.ojbkxc.hyx.core.HyXNative$LogCallback { *; }
-keep class com.ojbkxc.hyx.core.HyXNative$LogCallback { *; }

# HyXNative object — keep 所有成员，防止字段/方法被混淆影响 JNI 绑定。
-keep class com.ojbkxc.hyx.core.HyXNative { *; }
-keep class com.ojbkxc.hyx.core.HyXNative$* { *; }
