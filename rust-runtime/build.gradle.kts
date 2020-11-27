description = "Rust Runtime"
plugins {
    kotlin("jvm")
}

group = "software.amazon.rustruntime"

version = "1.0"

tasks.jar {
    from("./") {
        include("inlineable/src/*.rs")
        include("inlineable/Cargo.toml")
    }
}
