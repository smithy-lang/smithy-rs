function expandTable(event, id) {
    event.preventDefault();
    const table = document.querySelector(`#${id}`);
    table.classList.remove("hidden");

    const expander = document.querySelector(`#${id}-exp`);
    expander.classList.add("hidden");
}
