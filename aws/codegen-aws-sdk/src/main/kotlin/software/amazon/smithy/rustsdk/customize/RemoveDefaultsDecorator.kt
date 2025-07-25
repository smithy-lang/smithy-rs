/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
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
    private val removeDefaults: Map<ShapeId, Set<ShapeId>> =
        mapOf(
            "com.amazonaws.amplifyuibuilder#AmplifyUIBuilder" to
                setOf(
                    "com.amazonaws.amplifyuibuilder#ListComponentsLimit",
                    "com.amazonaws.amplifyuibuilder#ListFormsLimit",
                    "com.amazonaws.amplifyuibuilder#ListThemesLimit",
                ),
            "com.amazonaws.drs#ElasticDisasterRecoveryService" to
                setOf(
                    "com.amazonaws.drs#Validity",
                    "com.amazonaws.drs#CostOptimizationConfiguration\$burstBalanceThreshold",
                    "com.amazonaws.drs#CostOptimizationConfiguration\$burstBalanceDeltaThreshold",
                    "com.amazonaws.drs.synthetic#ListStagingAccountsInput\$maxResults",
                    "com.amazonaws.drs#StrictlyPositiveInteger",
                    "com.amazonaws.drs#MaxResultsType",
                    "com.amazonaws.drs#MaxResultsReplicatingSourceServers",
                    "com.amazonaws.drs#LaunchActionOrder",
                ),
            "com.amazonaws.evidently#Evidently" to
                setOf(
                    "com.amazonaws.evidently#ResultsPeriod",
                ),
            "com.amazonaws.location#LocationService" to
                setOf(
                    "com.amazonaws.location.synthetic#ListPlaceIndexesInput\$MaxResults",
                    "com.amazonaws.location.synthetic#SearchPlaceIndexForSuggestionsInput\$MaxResults",
                    "com.amazonaws.location#PlaceIndexSearchResultLimit",
                ),
            "com.amazonaws.paymentcryptographydata#PaymentCryptographyDataPlane" to
                setOf(
                    "com.amazonaws.paymentcryptographydata#IntegerRangeBetween4And12",
                ),
            "com.amazonaws.emrserverless#AwsToledoWebService" to
                setOf(
                    "com.amazonaws.emrserverless#WorkerCounts",
                ),
            "com.amazonaws.s3control#AWSS3ControlServiceV20180820" to
                setOf(
                    "com.amazonaws.s3control#PublicAccessBlockConfiguration\$BlockPublicAcls",
                    "com.amazonaws.s3control#PublicAccessBlockConfiguration\$IgnorePublicAcls",
                    "com.amazonaws.s3control#PublicAccessBlockConfiguration\$BlockPublicPolicy",
                    "com.amazonaws.s3control#PublicAccessBlockConfiguration\$RestrictPublicBuckets",
                ),
            "com.amazonaws.iot#AWSIotService" to
                setOf(
                    "com.amazonaws.iot#ThingConnectivity\$connected",
                    "com.amazonaws.iot.synthetic#UpdateProvisioningTemplateInput\$enabled",
                    "com.amazonaws.iot.synthetic#CreateProvisioningTemplateInput\$enabled",
                    "com.amazonaws.iot.synthetic#DescribeProvisioningTemplateOutput\$enabled",
                    "com.amazonaws.iot.synthetic#DescribeProvisioningTemplateOutput\$enabled",
                    "com.amazonaws.iot#ProvisioningTemplateSummary\$enabled",
                ),
        ).map { (k, v) -> k.shapeId() to v.map { it.shapeId() }.toSet() }.toMap()

    private fun applies(service: ServiceShape) = removeDefaults.containsKey(service.id)

    override fun transformModel(
        service: ServiceShape,
        model: Model,
        settings: ClientRustSettings,
    ): Model {
        if (!applies(service)) {
            return model
        }
        logger.info("Removing invalid defaults from ${service.id}")
        return RemoveDefaults.processModel(model, removeDefaults[service.id]!!)
    }
}
