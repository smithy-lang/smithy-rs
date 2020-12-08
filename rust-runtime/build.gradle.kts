description = "Rust Runtime"
plugins {
    kotlin("jvm")
}

group = "software.amazon.rustruntime"

version = "0.0.3"

tasks.jar {
    from("./") {
        include("inlineable/src/*.rs")
        include("inlineable/Cargo.toml")
    }
}
