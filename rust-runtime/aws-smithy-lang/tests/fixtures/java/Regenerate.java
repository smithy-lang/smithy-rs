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
import software.amazon.smithy.model.SourceLocation;
import software.amazon.smithy.model.loader.ModelAssembler;
import software.amazon.smithy.model.loader.Prelude;
import software.amazon.smithy.model.node.ArrayNode;
import software.amazon.smithy.model.node.Node;
import software.amazon.smithy.model.node.ObjectNode;
import software.amazon.smithy.model.shapes.EntityShape;
import software.amazon.smithy.model.shapes.ModelSerializer;
import software.amazon.smithy.model.shapes.ServiceShape;
import software.amazon.smithy.model.shapes.Shape;
import software.amazon.smithy.model.shapes.ShapeId;
import software.amazon.smithy.model.traits.DynamicTrait;
import software.amazon.smithy.model.traits.Trait;
import software.amazon.smithy.model.traits.TraitFactory;
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
 * <p>For every IDL fixture in {@code idl/GROUP/} (a {@code .smithy} file, or a directory of
 * {@code .smithy} and {@code .json} files assembled together), records the IDs of ERROR and
 * DANGER events separately, plus the location and message of the first ERROR event. Fixtures
 * without ERROR events are also written to {@code expected/idl/NAME.json}.
 *
 * <p>For every case in {@code assembly/} (a directory with a {@code case.json} listing assembler
 * steps), records the outcome and, for accepted cases, the serialized model and the source file
 * of selected shapes, members, and traits. These cover inputs added in code ({@code addShape},
 * {@code addModel}, {@code putMetadata}, {@code addTrait}), which file fixtures cannot express.
 *
 * <p>See README.md for how to run this.
 */
public final class Regenerate {
    /** The IDs of the prelude's own shapes. */
    private static final java.util.Set<ShapeId> PRELUDE_IDS = Model.assembler().assemble().unwrap().getShapeIds();

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

        regenerateIdl(root);
        regenerateAssembly(root);
    }

    private static void regenerateIdl(Path root) throws Exception {
        Path idlExpected = root.resolve("expected").resolve("idl");
        Files.createDirectories(idlExpected);
        TreeMap<String, Node> idlOutcomes = new TreeMap<>();
        for (String group : List.of("valid", "danger", "unsupported", "invalid")) {
            List<Path> fixtures = new ArrayList<>();
            try (Stream<Path> files = Files.list(root.resolve("idl").resolve(group))) {
                files.filter(p -> Files.isDirectory(p) || p.toString().endsWith(".smithy"))
                        .sorted()
                        .forEach(fixtures::add);
            }
            for (Path fixture : fixtures) {
                String stem = fixture.getFileName().toString().replace(".smithy", "");
                ValidatedResult<Model> result = importSorted(Model.assembler(), fixture).assemble();
                TreeSet<String> errors = new TreeSet<>();
                TreeSet<String> dangers = new TreeSet<>();
                ValidationEvent first = null;
                for (ValidationEvent event : result.getValidationEvents()) {
                    if (event.getSeverity() == Severity.ERROR) {
                        errors.add(event.getId());
                        if (first == null) {
                            first = event;
                        }
                    } else if (event.getSeverity() == Severity.DANGER) {
                        dangers.add(event.getId());
                    }
                }
                boolean accepted = errors.isEmpty() && result.getResult().isPresent();
                ObjectNode.Builder outcome = ObjectNode.builder()
                        .withMember("accepted", accepted)
                        .withMember("errors", errors.stream().map(Node::from).collect(ArrayNode.collect()))
                        .withMember("dangers", dangers.stream().map(Node::from).collect(ArrayNode.collect()));
                if (first != null) {
                    String file = first.getSourceLocation().getFilename();
                    String fileName = file.isEmpty() ? "" : Path.of(file).getFileName().toString();
                    outcome.withMember("firstError", ObjectNode.builder()
                            .withMember("file", fileName)
                            .withMember("line", first.getSourceLocation().getLine())
                            .withMember("column", first.getSourceLocation().getColumn())
                            .withMember("message", first.getMessage())
                            .build());
                }
                idlOutcomes.put(group + "/" + stem, outcome.build());
                if (accepted) {
                    // `unwrap` also rejects DANGER events, which these fixtures may contain.
                    Model model = result.getResult().get();
                    ObjectNode serialized = ModelSerializer.builder().build().serialize(model);
                    Files.writeString(idlExpected.resolve(stem + ".json"), Node.prettyPrintJson(serialized) + "\n");
                }
            }
        }
        ObjectNode.Builder cases = ObjectNode.builder();
        idlOutcomes.forEach(cases::withMember);
        ObjectNode document = ObjectNode.builder()
                .withMember("generator", "software.amazon.smithy:smithy-model:1.74.0")
                .withMember("outcomes", cases.build())
                .build();
        Files.writeString(idlExpected.resolve("outcomes.json"), Node.prettyPrintJson(document) + "\n");
    }

    private static void regenerateAssembly(Path root) throws Exception {
        Path expected = root.resolve("expected").resolve("assembly");
        Files.createDirectories(expected);
        TreeMap<String, Node> outcomes = new TreeMap<>();
        List<Path> cases = new ArrayList<>();
        try (Stream<Path> files = Files.list(root.resolve("assembly"))) {
            files.filter(Files::isDirectory).sorted().forEach(cases::add);
        }
        TraitFactory traitFactory = TraitFactory.createServiceFactory();
        for (Path dir : cases) {
            String name = dir.getFileName().toString();
            ObjectNode spec = Node.parse(Files.readString(dir.resolve("case.json"))).expectObjectNode();
            ObjectNode generate = spec.getObjectMember("serializeWithPrelude").orElse(null);
            if (generate != null) {
                // An input file generated from another one: the model serialized with its
                // prelude shapes, as the smithy-build `model` plugin writes it with
                // `includePreludeShapes`.
                Model source = Model.assembler()
                        .addImport(dir.resolve(generate.expectStringMember("from").getValue()))
                        .assemble()
                        .unwrap();
                ObjectNode serialized = ModelSerializer.builder().includePrelude(true).build().serialize(source);
                Files.writeString(dir.resolve(generate.expectStringMember("to").getValue()),
                        Node.prettyPrintJson(serialized) + "\n");
            }
            ModelAssembler assembler = Model.assembler();
            for (Node element : spec.expectArrayMember("steps").getElements()) {
                applyStep(assembler, dir, element.expectObjectNode(), traitFactory);
            }
            ValidatedResult<Model> result = assembler.assemble();
            TreeSet<String> errors = new TreeSet<>();
            for (ValidationEvent event : result.getValidationEvents()) {
                if (event.getSeverity() == Severity.ERROR || event.getSeverity() == Severity.DANGER) {
                    errors.add(event.getId());
                }
            }
            boolean accepted = errors.isEmpty() && result.getResult().isPresent();
            ObjectNode.Builder outcome = ObjectNode.builder()
                    .withMember("accepted", accepted)
                    .withMember("errors", errors.stream().map(Node::from).collect(ArrayNode.collect()));
            if (accepted) {
                Model model = result.getResult().get();
                ObjectNode.Builder locations = ObjectNode.builder();
                for (Node key : spec.getArrayMember("locations").map(ArrayNode::getElements)
                        .orElse(List.of())) {
                    String target = key.expectStringNode().getValue();
                    locations.withMember(target, sourceFile(model, target));
                }
                outcome.withMember("locations", locations.build());
                // Shapes a model adds to the prelude namespace are kept; only the prelude's own
                // shapes are left out.
                ObjectNode serialized = ModelSerializer.builder()
                        .includePrelude(true)
                        .shapeFilter(shape -> !PRELUDE_IDS.contains(shape.getId().withoutMember()))
                        .build()
                        .serialize(model);
                Files.writeString(expected.resolve(name + ".json"), Node.prettyPrintJson(serialized) + "\n");
            }
            outcomes.put(name, outcome.build());
        }
        ObjectNode.Builder caseNodes = ObjectNode.builder();
        outcomes.forEach(caseNodes::withMember);
        ObjectNode document = ObjectNode.builder()
                .withMember("generator", "software.amazon.smithy:smithy-model:1.74.0")
                .withMember("outcomes", caseNodes.build())
                .build();
        Files.writeString(expected.resolve("outcomes.json"), Node.prettyPrintJson(document) + "\n");
    }

    /** Applies one {@code case.json} step to {@code assembler}. Paths are relative to the case. */
    private static void applyStep(ModelAssembler assembler, Path dir, ObjectNode step, TraitFactory traitFactory) {
        if (step.getStringMember("addShapes").isPresent()) {
            // The shapes of a file, added one by one as if built in code.
            boolean disablePrelude = step.getBooleanMemberOrDefault("disablePrelude", false);
            ModelAssembler loader = Model.assembler();
            if (disablePrelude) {
                loader.disablePrelude();
            }
            Model source = loader.addImport(dir.resolve(step.expectStringMember("addShapes").getValue()))
                    .assemble().unwrap();
            for (Shape shape : source.toSet()) {
                if (!shape.isMemberShape() && (disablePrelude || !Prelude.isPreludeShape(shape))) {
                    assembler.addShape(shape);
                }
            }
        } else if (step.getStringMember("addModel").isPresent()) {
            Path file = dir.resolve(step.expectStringMember("addModel").getValue());
            assembler.addModel(Model.assembler().addImport(file).assemble().unwrap());
        } else if (step.getStringMember("addFile").isPresent()) {
            assembler.addImport(dir.resolve(step.expectStringMember("addFile").getValue()));
        } else if (step.getStringMember("putMetadata").isPresent()) {
            assembler.putMetadata(step.expectStringMember("putMetadata").getValue(), step.expectMember("value"));
        } else if (step.getStringMember("addTrait").isPresent()) {
            ShapeId target = ShapeId.from(step.expectStringMember("addTrait").getValue());
            ShapeId traitId = ShapeId.from(step.expectStringMember("trait").getValue());
            Node value = step.expectMember("value");
            Trait trait = traitFactory.createTrait(traitId, target, value)
                    .orElseGet(() -> new DynamicTrait(traitId, value));
            assembler.addTrait(target, trait);
        } else {
            throw new IllegalArgumentException("Unknown step in " + dir + ": " + Node.printJson(step));
        }
    }

    /**
     * The file name a shape, member, or trait ({@code SHAPE_ID@TRAIT_ID}) came from, or an empty
     * string for one created in code.
     */
    private static String sourceFile(Model model, String target) {
        String[] parts = target.split("@", 2);
        Shape shape = model.expectShape(ShapeId.from(parts[0]));
        SourceLocation location = parts.length == 1
                ? shape.getSourceLocation()
                : shape.findTrait(ShapeId.from(parts[1])).orElseThrow().getSourceLocation();
        if (location == SourceLocation.NONE || location.getFilename().isEmpty()
                || location.getFilename().equals("N/A")) {
            return "";
        }
        return Path.of(location.getFilename()).getFileName().toString();
    }

    /**
     * Imports a fixture file, or the files of a fixture directory in sorted order, matching
     * the order the Rust harness adds them. {@code addImport} on a directory follows the file
     * system's order, which decides the order of merged trait arrays.
     */
    private static ModelAssembler importSorted(ModelAssembler assembler, Path fixture) throws Exception {
        if (!Files.isDirectory(fixture)) {
            return assembler.addImport(fixture);
        }
        try (Stream<Path> files = Files.list(fixture)) {
            files.sorted().forEach(assembler::addImport);
        }
        return assembler;
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
