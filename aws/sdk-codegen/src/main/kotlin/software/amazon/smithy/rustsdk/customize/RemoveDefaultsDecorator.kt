/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustSettings
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.util.shapeId
import java.util.logging.Logger

/**
 * Removes default values from certain shapes, and any member that targets those shapes,
 * for some services where the default value causes serialization issues, validation
 * issues, or other unexpected behavior.
 */
class RemoveDefaultsDecorator : ClientCodegenDecorator {
    override val name: String = "RemoveDefaults"
    override val order: Byte = 0
    private val logger: Logger = Logger.getLogger(javaClass.name)

    // Service shape id -> Shape id of each root shape to remove the default from.
    // TODO(https://github.com/smithy-lang/smithy-rs/issues/3220): Remove this customization after model updates.
    private val removeDefaults = mapOf(
        "com.amazonaws.amplifyuibuilder#AmplifyUIBuilder".shapeId() to setOf(
            "com.amazonaws.amplifyuibuilder#ListComponentsLimit".shapeId(),
            "com.amazonaws.amplifyuibuilder#ListFormsLimit".shapeId(),
            "com.amazonaws.amplifyuibuilder#ListThemesLimit".shapeId(),
        ),
        "com.amazonaws.drs#ElasticDisasterRecoveryService".shapeId() to setOf(
            "com.amazonaws.drs#Validity".shapeId(),
            "com.amazonaws.drs#CostOptimizationConfiguration\$burstBalanceThreshold".shapeId(),
            "com.amazonaws.drs#CostOptimizationConfiguration\$burstBalanceDeltaThreshold".shapeId(),
            "com.amazonaws.drs#ListStagingAccountsRequest\$maxResults".shapeId(),
            "com.amazonaws.drs#StrictlyPositiveInteger".shapeId(),
            "com.amazonaws.drs#MaxResultsType".shapeId(),
            "com.amazonaws.drs#MaxResultsReplicatingSourceServers".shapeId(),
            "com.amazonaws.drs#LaunchActionOrder".shapeId(),
        ),
        "com.amazonaws.evidently#Evidently".shapeId() to setOf(
            "com.amazonaws.evidently#ResultsPeriod".shapeId(),
        ),
        "com.amazonaws.location#LocationService".shapeId() to setOf(
            "com.amazonaws.location#ListPlaceIndexesRequest\$MaxResults".shapeId(),
            "com.amazonaws.location#SearchPlaceIndexForSuggestionsRequest\$MaxResults".shapeId(),
            "com.amazonaws.location#PlaceIndexSearchResultLimit".shapeId(),
        ),
        "com.amazonaws.paymentcryptographydata#PaymentCryptographyDataPlane".shapeId() to setOf(
            "com.amazonaws.paymentcryptographydata#IntegerRangeBetween4And12".shapeId(),
        ),
        "com.amazonaws.emrserverless#AwsToledoWebService".shapeId() to setOf(
            // Service expects this to have a min value > 0
            "com.amazonaws.emrserverless#WorkerCounts".shapeId(),
        ),
    )

    private fun applies(service: ServiceShape) =
        removeDefaults.containsKey(service.id)

    override fun transformModel(service: ServiceShape, model: Model, settings: ClientRustSettings): Model {
        if (!applies(service)) {
            return model
        }
        logger.info("Removing invalid defaults from ${service.id}")
        return RemoveDefaults.processModel(model, removeDefaults[service.id]!!)
    }
}
