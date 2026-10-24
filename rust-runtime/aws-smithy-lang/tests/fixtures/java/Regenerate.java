/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collection;
import java.util.Comparator;
import java.util.List;
import java.util.TreeMap;
import java.util.TreeSet;
import java.util.function.Function;
import java.util.stream.Collectors;
import java.util.stream.Stream;
import software.amazon.smithy.model.Model;
import software.amazon.smithy.model.knowledge.BottomUpIndex;
import software.amazon.smithy.model.knowledge.OperationIndex;
import software.amazon.smithy.model.knowledge.TopDownIndex;
import software.amazon.smithy.model.node.ArrayNode;
import software.amazon.smithy.model.node.Node;
import software.amazon.smithy.model.node.ObjectNode;
import software.amazon.smithy.model.shapes.EntityShape;
import software.amazon.smithy.model.shapes.ModelSerializer;
import software.amazon.smithy.model.shapes.ServiceShape;
import software.amazon.smithy.model.shapes.Shape;
import software.amazon.smithy.model.validation.Severity;
import software.amazon.smithy.model.validation.ValidatedResult;
import software.amazon.smithy.model.validation.ValidationEvent;

/**
 * Regenerates the expected outcomes for the Java compatibility fixtures.
 *
 * <p>For every fixture in {@code valid/} and {@code invalid/}, records whether Java Smithy
 * accepts it and the IDs of any ERROR or DANGER events. For accepted fixtures, also writes the
 * model as serialized by Java's {@code ModelSerializer} to {@code expected/NAME.json}.
 *
 * <p>For every fixture in {@code knowledge/}, writes the answers of Java's structural knowledge
 * indexes to {@code expected/knowledge/NAME.json}.
 *
 * <p>See README.md for how to run this.
 */
public final class Regenerate {
    public static void main(String[] args) throws Exception {
        Path root = Path.of(args.length > 0 ? args[0] : ".");
        Path expected = root.resolve("expected");
        Files.createDirectories(expected);
        TreeMap<String, Node> outcomes = new TreeMap<>();
        for (String group : List.of("valid", "invalid")) {
            List<Path> fixtures = new ArrayList<>();
            try (Stream<Path> files = Files.list(root.resolve(group))) {
                files.filter(p -> p.toString().endsWith(".json")).sorted().forEach(fixtures::add);
            }
            for (Path fixture : fixtures) {
                String name = group + "/" + fixture.getFileName().toString().replace(".json", "");
                ValidatedResult<Model> result = Model.assembler().addImport(fixture).assemble();
                TreeSet<String> errors = new TreeSet<>();
                for (ValidationEvent event : result.getValidationEvents()) {
                    if (event.getSeverity() == Severity.ERROR || event.getSeverity() == Severity.DANGER) {
                        errors.add(event.getId());
                    }
                }
                boolean accepted = errors.isEmpty() && result.getResult().isPresent();
                outcomes.put(name, ObjectNode.builder()
                        .withMember("accepted", accepted)
                        .withMember("errors", errors.stream().map(Node::from).collect(ArrayNode.collect()))
                        .build());
                if (accepted) {
                    ObjectNode serialized = ModelSerializer.builder().build().serialize(result.unwrap());
                    String file = fixture.getFileName().toString();
                    Files.writeString(expected.resolve(file), Node.prettyPrintJson(serialized) + "\n");
                }
            }
        }
        ObjectNode.Builder document = ObjectNode.builder()
                .withMember("generator", "software.amazon.smithy:smithy-model:1.74.0");
        ObjectNode.Builder cases = ObjectNode.builder();
        outcomes.forEach(cases::withMember);
        document.withMember("outcomes", cases.build());
        Files.writeString(expected.resolve("outcomes.json"), Node.prettyPrintJson(document.build()) + "\n");

        Path knowledgeExpected = expected.resolve("knowledge");
        Files.createDirectories(knowledgeExpected);
        List<Path> knowledgeFixtures = new ArrayList<>();
        try (Stream<Path> files = Files.list(root.resolve("knowledge"))) {
            files.filter(p -> p.toString().endsWith(".json")).sorted().forEach(knowledgeFixtures::add);
        }
        for (Path fixture : knowledgeFixtures) {
            // Knowledge fixtures exercise structural edge cases (such as lifecycle operations
            // without identifier bindings) that Java's full validator rejects.
            Model model = Model.assembler().addImport(fixture).disableValidation().assemble().unwrap();
            String file = fixture.getFileName().toString();
            Files.writeString(knowledgeExpected.resolve(file), Node.prettyPrintJson(knowledge(model)) + "\n");
        }
    }

    private static ArrayNode ids(Collection<? extends Shape> shapes) {
        return shapes.stream().map(s -> Node.from(s.getId().toString())).collect(ArrayNode.collect());
    }

    private static ArrayNode sortedIds(Collection<? extends Shape> shapes) {
        return shapes.stream().map(s -> s.getId().toString()).sorted().map(Node::from).collect(ArrayNode.collect());
    }

    private static <T extends Shape> ObjectNode byId(Stream<T> shapes, Function<T, Node> value) {
        TreeMap<String, Node> entries = new TreeMap<>();
        shapes.forEach(shape -> entries.put(shape.getId().toString(), value.apply(shape)));
        ObjectNode.Builder builder = ObjectNode.builder();
        entries.forEach(builder::withMember);
        return builder.build();
    }

    private static Node knowledge(Model model) {
        TopDownIndex topDown = TopDownIndex.of(model);
        OperationIndex operations = OperationIndex.of(model);
        BottomUpIndex bottomUp = BottomUpIndex.of(model);
        Stream<EntityShape> entities = Stream.concat(
                model.getServiceShapes().stream(), model.getResourceShapes().stream());
        List<ServiceShape> services = model.getServiceShapes().stream()
                .sorted(Comparator.comparing(Shape::getId)).collect(Collectors.toList());

        ObjectNode topDownNode = byId(entities, entity -> ObjectNode.builder()
                .withMember("operations", ids(topDown.getContainedOperations(entity)))
                .withMember("resources", ids(topDown.getContainedResources(entity)))
                .build());

        ObjectNode operationsNode = byId(model.getOperationShapes().stream(), operation -> {
            ObjectNode.Builder errorsIn = ObjectNode.builder();
            for (ServiceShape service : services) {
                if (topDown.getContainedOperations(service).contains(operation)) {
                    errorsIn.withMember(service.getId().toString(), ids(operations.getErrors(service, operation)));
                }
            }
            return ObjectNode.builder()
                    .withMember("input", operations.expectInputShape(operation).getId().toString())
                    .withMember("output", operations.expectOutputShape(operation).getId().toString())
                    .withMember("meaningfulInput", operations.getInput(operation)
                            .<Node>map(s -> Node.from(s.getId().toString())).orElse(Node.nullNode()))
                    .withMember("meaningfulOutput", operations.getOutput(operation)
                            .<Node>map(s -> Node.from(s.getId().toString())).orElse(Node.nullNode()))
                    .withMember("inputMembers", ids(operations.getInputMembers(operation).values()))
                    .withMember("outputMembers", ids(operations.getOutputMembers(operation).values()))
                    .withMember("directErrors", ids(operations.getErrors(operation)))
                    .withMember("errorsIn", errorsIn.build())
                    .build();
        });

        ObjectNode structuresNode = byId(model.getStructureShapes().stream(), structure -> ObjectNode.builder()
                .withMember("inputBindings", sortedIds(operations.getInputBindings(structure)))
                .withMember("outputBindings", sortedIds(operations.getOutputBindings(structure)))
                .withMember("errorBindings", sortedIds(operations.getErrorBindings(structure)))
                .withMember("isInput", operations.isInputStructure(structure))
                .withMember("isOutput", operations.isOutputStructure(structure))
                .build());

        // Java records one parent path per entity even when several exist; Rust only compares
        // the paths it can prove are unique.
        ObjectNode.Builder bottomUpNode = ObjectNode.builder();
        for (ServiceShape service : services) {
            Stream<Shape> contained = Stream.concat(
                    topDown.getContainedOperations(service).stream(),
                    topDown.getContainedResources(service).stream());
            bottomUpNode.withMember(service.getId().toString(),
                    byId(contained, entity -> ids(bottomUp.getAllParents(service, entity))));
        }

        return ObjectNode.builder()
                .withMember("generator", "software.amazon.smithy:smithy-model:1.74.0")
                .withMember("topDown", topDownNode)
                .withMember("operations", operationsNode)
                .withMember("structures", structuresNode)
                .withMember("bottomUp", bottomUpNode.build())
                .build();
    }
}
