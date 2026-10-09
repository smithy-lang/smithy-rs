/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.TreeMap;
import java.util.TreeSet;
import java.util.stream.Stream;
import software.amazon.smithy.model.Model;
import software.amazon.smithy.model.node.ArrayNode;
import software.amazon.smithy.model.node.Node;
import software.amazon.smithy.model.node.ObjectNode;
import software.amazon.smithy.model.shapes.ModelSerializer;
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
    }
}
