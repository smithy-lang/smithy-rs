/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.traits;

import org.junit.jupiter.api.Test;
import software.amazon.smithy.model.SourceLocation;
import software.amazon.smithy.model.shapes.ShapeId;

import static org.junit.jupiter.api.Assertions.assertEquals;

public class CacheableTraitTest {

  @Test
  public void testCacheableTrait() {
    CacheableTrait trait = new CacheableTrait(SourceLocation.NONE);
    assertEquals(ShapeId.from("smithy.rust#cacheable"), trait.toShapeId());

    // Test the Provider
    CacheableTrait.Provider provider = new CacheableTrait.Provider();
    assertEquals(ShapeId.from("smithy.rust#cacheable"), provider.getShapeId());
  }
}
