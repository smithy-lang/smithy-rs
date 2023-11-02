/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

mod authz;
mod plugin;

use std::{net::SocketAddr, sync::Arc};

use aws_smithy_http_server::{
    extension::OperationExtensionExt,
    instrumentation::InstrumentExt,
    layer::alb_health_check::AlbHealthCheckLayer,
    plugin::{HttpPlugins, ModelPlugins, Scoped},
    request::request_id::ServerRequestIdProviderLayer,
    AddExtensionLayer,
};
use clap::Parser;

use hyper::StatusCode;
use plugin::PrintExt;

use pokemon_service::{
    do_nothing_but_log_request_ids, get_storage_with_local_approved, DEFAULT_ADDRESS, DEFAULT_PORT,
};
use pokemon_service_common::{
    capture_pokemon, check_health, get_pokemon_species, get_server_statistics, setup_tracing,
    stream_pokemon_radio, State,
};
use pokemon_service_server_sdk::{scope, PokemonService, PokemonServiceConfig};

use crate::authz::AuthorizationPlugin;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Hyper server bind address.
    #[clap(short, long, action, default_value = DEFAULT_ADDRESS)]
    address: String,
    /// Hyper server bind port.
    #[clap(short, long, action, default_value_t = DEFAULT_PORT)]
    port: u16,
}

#[tokio::main]
async fn main() {
        let args = Args::parse();
        setup_tracing();

        //scope! {
        //    /// A scope containing `GetPokemonSpecies` and `GetStorage`.
        //    struct PrintScope {
        //        includes: [GetPokemonSpecies, GetStorage]
        //    }
        //}
        // Scope the `PrintPlugin`, defined in `plugin.rs`, to `PrintScope`.
    //let print_plugin = Scoped::new::<PrintScope>(HttpPlugins::new().print());


    let http_plugins = HttpPlugins::new()
        // Apply the scoped `PrintPlugin`
        //.push(print_plugin)
        // Apply the `OperationExtensionPlugin` defined in `aws_smithy_http_server::extension`. This allows other
        // plugins or tests to access a `aws_smithy_http_server::extension::OperationExtension` from
        // `Response::extensions`, or infer routing failure when it's missing.
        .insert_operation_extension()
        // Adds `tracing` spans and events to the request lifecycle.
        .instrument();

    let authz_plugin = AuthorizationPlugin::new();
    let model_plugins = ModelPlugins::new().push(authz_plugin);

    let config = PokemonServiceConfig::builder()
        // Set up shared state and middlewares.
        .layer(AddExtensionLayer::new(Arc::new(State::default())))
        // Handle `/ping` health check requests.
        .layer(AlbHealthCheckLayer::from_handler("/ping", |_req| async {
            StatusCode::OK
        }))
        // Add server request IDs.
        .layer(ServerRequestIdProviderLayer::new())
        .http_plugin(http_plugins)
        .model_plugin(model_plugins)
        .build();

    let app = PokemonService::builder(config)
        // Build a registry containing implementations to all the operations in the service. These
        // are async functions or async closures that take as input the operation's input and
        // return the operation's output.
        .get_pokemon_species(get_pokemon_species)
        .get_storage(get_storage_with_local_approved)
        .get_server_statistics(get_server_statistics)
        .capture_pokemon(capture_pokemon)
        .do_nothing(do_nothing_but_log_request_ids)
        .check_health(check_health)
        .stream_pokemon_radio(stream_pokemon_radio)
        .custom_op0(pokemon_service_common::get_custom_op0)
        .custom_op1(pokemon_service_common::get_custom_op1)
        .custom_op2(pokemon_service_common::get_custom_op2)
        .custom_op3(pokemon_service_common::get_custom_op3)
        .custom_op4(pokemon_service_common::get_custom_op4)
        .custom_op5(pokemon_service_common::get_custom_op5)
        .custom_op6(pokemon_service_common::get_custom_op6)
        .custom_op7(pokemon_service_common::get_custom_op7)
        .custom_op8(pokemon_service_common::get_custom_op8)
        .custom_op9(pokemon_service_common::get_custom_op9)
        .custom_op10(pokemon_service_common::get_custom_op10)
        .custom_op11(pokemon_service_common::get_custom_op11)
        .custom_op12(pokemon_service_common::get_custom_op12)
        .custom_op13(pokemon_service_common::get_custom_op13)
        .custom_op14(pokemon_service_common::get_custom_op14)
        .custom_op15(pokemon_service_common::get_custom_op15)
        .custom_op16(pokemon_service_common::get_custom_op16)
        .custom_op17(pokemon_service_common::get_custom_op17)
        .custom_op18(pokemon_service_common::get_custom_op18)
        .custom_op19(pokemon_service_common::get_custom_op19)
        .custom_op20(pokemon_service_common::get_custom_op20)
        .custom_op21(pokemon_service_common::get_custom_op21)
        .custom_op22(pokemon_service_common::get_custom_op22)
        .custom_op23(pokemon_service_common::get_custom_op23)
        .custom_op24(pokemon_service_common::get_custom_op24)
        .custom_op25(pokemon_service_common::get_custom_op25)
        .custom_op26(pokemon_service_common::get_custom_op26)
        .custom_op27(pokemon_service_common::get_custom_op27)
        .custom_op28(pokemon_service_common::get_custom_op28)
        .custom_op29(pokemon_service_common::get_custom_op29)
        .custom_op30(pokemon_service_common::get_custom_op30)
        .custom_op31(pokemon_service_common::get_custom_op31)
        .custom_op32(pokemon_service_common::get_custom_op32)
        .custom_op33(pokemon_service_common::get_custom_op33)
        .custom_op34(pokemon_service_common::get_custom_op34)
        .custom_op35(pokemon_service_common::get_custom_op35)
        .custom_op36(pokemon_service_common::get_custom_op36)
        .custom_op37(pokemon_service_common::get_custom_op37)
        .custom_op38(pokemon_service_common::get_custom_op38)
        .custom_op39(pokemon_service_common::get_custom_op39)
        .custom_op40(pokemon_service_common::get_custom_op40)
        .custom_op41(pokemon_service_common::get_custom_op41)
        .custom_op42(pokemon_service_common::get_custom_op42)
        .custom_op43(pokemon_service_common::get_custom_op43)
        .custom_op44(pokemon_service_common::get_custom_op44)
        .custom_op45(pokemon_service_common::get_custom_op45)
        .custom_op46(pokemon_service_common::get_custom_op46)
        .custom_op47(pokemon_service_common::get_custom_op47)
        .custom_op48(pokemon_service_common::get_custom_op48)
        .custom_op49(pokemon_service_common::get_custom_op49)
        .custom_op50(pokemon_service_common::get_custom_op50)
        .custom_op51(pokemon_service_common::get_custom_op51)
        .custom_op52(pokemon_service_common::get_custom_op52)
        .custom_op53(pokemon_service_common::get_custom_op53)
        .custom_op54(pokemon_service_common::get_custom_op54)
        .custom_op55(pokemon_service_common::get_custom_op55)
        .custom_op56(pokemon_service_common::get_custom_op56)
        .custom_op57(pokemon_service_common::get_custom_op57)
        .custom_op58(pokemon_service_common::get_custom_op58)
        .custom_op59(pokemon_service_common::get_custom_op59)
        .custom_op60(pokemon_service_common::get_custom_op60)
        .custom_op61(pokemon_service_common::get_custom_op61)
        .custom_op62(pokemon_service_common::get_custom_op62)
        .custom_op63(pokemon_service_common::get_custom_op63)
        .custom_op64(pokemon_service_common::get_custom_op64)
        .custom_op65(pokemon_service_common::get_custom_op65)
        .custom_op66(pokemon_service_common::get_custom_op66)
        .custom_op67(pokemon_service_common::get_custom_op67)
        .custom_op68(pokemon_service_common::get_custom_op68)
        .custom_op69(pokemon_service_common::get_custom_op69)
        .custom_op70(pokemon_service_common::get_custom_op70)
        .custom_op71(pokemon_service_common::get_custom_op71)
        .custom_op72(pokemon_service_common::get_custom_op72)
        .custom_op73(pokemon_service_common::get_custom_op73)
        .custom_op74(pokemon_service_common::get_custom_op74)
        .custom_op75(pokemon_service_common::get_custom_op75)
        .custom_op76(pokemon_service_common::get_custom_op76)
        .custom_op77(pokemon_service_common::get_custom_op77)
        .custom_op78(pokemon_service_common::get_custom_op78)
        .custom_op79(pokemon_service_common::get_custom_op79)
        .custom_op80(pokemon_service_common::get_custom_op80)
        .custom_op81(pokemon_service_common::get_custom_op81)
        .custom_op82(pokemon_service_common::get_custom_op82)
        .custom_op83(pokemon_service_common::get_custom_op83)
        .custom_op84(pokemon_service_common::get_custom_op84)
        .custom_op85(pokemon_service_common::get_custom_op85)
        .custom_op86(pokemon_service_common::get_custom_op86)
        .custom_op87(pokemon_service_common::get_custom_op87)
        .custom_op88(pokemon_service_common::get_custom_op88)
        .custom_op89(pokemon_service_common::get_custom_op89)
        .custom_op90(pokemon_service_common::get_custom_op90)
        .custom_op91(pokemon_service_common::get_custom_op91)
        .custom_op92(pokemon_service_common::get_custom_op92)
        .custom_op93(pokemon_service_common::get_custom_op93)
        .custom_op94(pokemon_service_common::get_custom_op94)
        .custom_op95(pokemon_service_common::get_custom_op95)
        .custom_op96(pokemon_service_common::get_custom_op96)
        .custom_op97(pokemon_service_common::get_custom_op97)
        .custom_op98(pokemon_service_common::get_custom_op98)
        .custom_op99(pokemon_service_common::get_custom_op99)
        .custom_op100(pokemon_service_common::get_custom_op100)
        .custom_op101(pokemon_service_common::get_custom_op101)
        .custom_op102(pokemon_service_common::get_custom_op102)
        .custom_op103(pokemon_service_common::get_custom_op103)
        .custom_op104(pokemon_service_common::get_custom_op104)
        .custom_op105(pokemon_service_common::get_custom_op105)
        .custom_op106(pokemon_service_common::get_custom_op106)
        .custom_op107(pokemon_service_common::get_custom_op107)
        .custom_op108(pokemon_service_common::get_custom_op108)
        .custom_op109(pokemon_service_common::get_custom_op109)
        .custom_op110(pokemon_service_common::get_custom_op110)
        .custom_op111(pokemon_service_common::get_custom_op111)
        .custom_op112(pokemon_service_common::get_custom_op112)
        .custom_op113(pokemon_service_common::get_custom_op113)
        .custom_op114(pokemon_service_common::get_custom_op114)
        .custom_op115(pokemon_service_common::get_custom_op115)
        .custom_op116(pokemon_service_common::get_custom_op116)
        .custom_op117(pokemon_service_common::get_custom_op117)
        .custom_op118(pokemon_service_common::get_custom_op118)
        .custom_op119(pokemon_service_common::get_custom_op119)
        .custom_op120(pokemon_service_common::get_custom_op120)
        .custom_op121(pokemon_service_common::get_custom_op121)
        .custom_op122(pokemon_service_common::get_custom_op122)
        .custom_op123(pokemon_service_common::get_custom_op123)
        .custom_op124(pokemon_service_common::get_custom_op124)
        .custom_op125(pokemon_service_common::get_custom_op125)
        .custom_op126(pokemon_service_common::get_custom_op126)
        .custom_op127(pokemon_service_common::get_custom_op127)
        .custom_op128(pokemon_service_common::get_custom_op128)
        .custom_op129(pokemon_service_common::get_custom_op129)
        .custom_op130(pokemon_service_common::get_custom_op130)
        .custom_op131(pokemon_service_common::get_custom_op131)
        .custom_op132(pokemon_service_common::get_custom_op132)
        .custom_op133(pokemon_service_common::get_custom_op133)
        .custom_op134(pokemon_service_common::get_custom_op134)
        .custom_op135(pokemon_service_common::get_custom_op135)
        .custom_op136(pokemon_service_common::get_custom_op136)
        .custom_op137(pokemon_service_common::get_custom_op137)
        .custom_op138(pokemon_service_common::get_custom_op138)
        .custom_op139(pokemon_service_common::get_custom_op139)
        .custom_op140(pokemon_service_common::get_custom_op140)
        .custom_op141(pokemon_service_common::get_custom_op141)
        .custom_op142(pokemon_service_common::get_custom_op142)
        .custom_op143(pokemon_service_common::get_custom_op143)
        .custom_op144(pokemon_service_common::get_custom_op144)
        .custom_op145(pokemon_service_common::get_custom_op145)
        .custom_op146(pokemon_service_common::get_custom_op146)
        .custom_op147(pokemon_service_common::get_custom_op147)
        .custom_op148(pokemon_service_common::get_custom_op148)
        .custom_op149(pokemon_service_common::get_custom_op149)
        .custom_op150(pokemon_service_common::get_custom_op150)
        .custom_op151(pokemon_service_common::get_custom_op151)
        .custom_op152(pokemon_service_common::get_custom_op152)
        .custom_op153(pokemon_service_common::get_custom_op153)
        .custom_op154(pokemon_service_common::get_custom_op154)
        .custom_op155(pokemon_service_common::get_custom_op155)
        .custom_op156(pokemon_service_common::get_custom_op156)
        .custom_op157(pokemon_service_common::get_custom_op157)
        .custom_op158(pokemon_service_common::get_custom_op158)
        .custom_op159(pokemon_service_common::get_custom_op159)
        .custom_op160(pokemon_service_common::get_custom_op160)
        .custom_op161(pokemon_service_common::get_custom_op161)
        .custom_op162(pokemon_service_common::get_custom_op162)
        .custom_op163(pokemon_service_common::get_custom_op163)
        .custom_op164(pokemon_service_common::get_custom_op164)
        .custom_op165(pokemon_service_common::get_custom_op165)
        .custom_op166(pokemon_service_common::get_custom_op166)
        .custom_op167(pokemon_service_common::get_custom_op167)
        .custom_op168(pokemon_service_common::get_custom_op168)
        .custom_op169(pokemon_service_common::get_custom_op169)
        .custom_op170(pokemon_service_common::get_custom_op170)
        .custom_op171(pokemon_service_common::get_custom_op171)
        .custom_op172(pokemon_service_common::get_custom_op172)
        .custom_op173(pokemon_service_common::get_custom_op173)
        .custom_op174(pokemon_service_common::get_custom_op174)
        .custom_op175(pokemon_service_common::get_custom_op175)
        .custom_op176(pokemon_service_common::get_custom_op176)
        .custom_op177(pokemon_service_common::get_custom_op177)
        .custom_op178(pokemon_service_common::get_custom_op178)
        .custom_op179(pokemon_service_common::get_custom_op179)
        .custom_op180(pokemon_service_common::get_custom_op180)
        .custom_op181(pokemon_service_common::get_custom_op181)
        .custom_op182(pokemon_service_common::get_custom_op182)
        .custom_op183(pokemon_service_common::get_custom_op183)
        .custom_op184(pokemon_service_common::get_custom_op184)
        .custom_op185(pokemon_service_common::get_custom_op185)
        .custom_op186(pokemon_service_common::get_custom_op186)
        .custom_op187(pokemon_service_common::get_custom_op187)
        .custom_op188(pokemon_service_common::get_custom_op188)
        .custom_op189(pokemon_service_common::get_custom_op189)
        .custom_op190(pokemon_service_common::get_custom_op190)
        .custom_op191(pokemon_service_common::get_custom_op191)
        .custom_op192(pokemon_service_common::get_custom_op192)
        .custom_op193(pokemon_service_common::get_custom_op193)
        .custom_op194(pokemon_service_common::get_custom_op194)
        .custom_op195(pokemon_service_common::get_custom_op195)
        .custom_op196(pokemon_service_common::get_custom_op196)
        .custom_op197(pokemon_service_common::get_custom_op197)
        .custom_op198(pokemon_service_common::get_custom_op198)
        .custom_op199(pokemon_service_common::get_custom_op199)
                .build()
        .expect("failed to build an instance of PokemonService");

    // Using `into_make_service_with_connect_info`, rather than `into_make_service`, to adjoin the `SocketAddr`
    // connection info.
    let make_app = app.into_make_service_with_connect_info::<SocketAddr>();

    // Bind the application to a socket.
    let bind: SocketAddr = format!("{}:{}", args.address, args.port)
        .parse()
        .expect("unable to parse the server bind address and port");
    let server = hyper::Server::bind(&bind).serve(make_app);

    // Run forever-ish...
    if let Err(err) = server.await {
        eprintln!("server error: {}", err);
    }
}

