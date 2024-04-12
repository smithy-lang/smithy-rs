/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

function expandTable(event, id) {
    event.preventDefault();
    const table = document.querySelector(`#${id}`);
    table.classList.remove("hidden");

    const expander = document.querySelector(`#${id}-exp`);
    expander.classList.add("hidden");
}
