#!/usr/bin/env python3
"""
Convert reagents.json from the original object-keyed format to our array format.

Original format:
  - Objects keyed by string IDs: {"1": {...}, "2": {...}}
  - IDs are 1-indexed

Our format:
  - Arrays: [{...}, {...}]
  - IDs are 0-indexed
  - All ID references are decremented by 1
"""

import json
import sys
from pathlib import Path


def convert_object_to_array(obj: dict) -> list:
    """
    Convert {"1": {...}, "2": {...}} to [{...}, {...}]
    Sorted by numeric key.
    """
    sorted_keys = sorted(obj.keys(), key=lambda k: int(k))
    return [obj[k] for k in sorted_keys]


def decrement_id(value: int) -> int:
    """Decrement an ID by 1 (convert from 1-indexed to 0-indexed)"""
    return value - 1


def convert_color(color: dict) -> dict:
    """Convert a color object, decrementing IDs"""
    return {
        "hex": color["hex"],
        "id": decrement_id(color["id"]),
        "name": color["name"],
        "simple": color["simple"],
        "simpleColorId": decrement_id(color["simpleColorId"]),
    }


def convert_reagent(reagent: dict) -> dict:
    """Convert a reagent object, decrementing IDs"""
    result = {
        "fullName": reagent["fullName"],
        "id": decrement_id(reagent["id"]),
        "name": reagent["name"],
        "shortName": reagent["shortName"],
    }
    if "whiteFirstColor" in reagent:
        result["whiteFirstColor"] = reagent["whiteFirstColor"]
    return result


def convert_substance(substance: dict) -> dict:
    """Convert a substance object, decrementing IDs"""
    return {
        "commonName": substance["commonName"],
        "id": decrement_id(substance["id"]),
        "isPopular": substance["isPopular"],
        "name": substance["name"],
        "token": substance["token"],
        "sid": substance["sid"],
        "classes": substance.get("classes", []),
    }


def convert_result_entry(entry):
    """
    Convert a result entry: [[start_colors], [end_colors], is_positive, description]
    Decrement all color IDs.
    """
    if entry is None:
        return None

    start_colors, end_colors, is_positive, description = entry

    return [
        [decrement_id(c) for c in start_colors] if start_colors else [],
        [decrement_id(c) for c in end_colors] if end_colors else [],
        is_positive,
        description,
    ]


def convert_results(results_obj: dict, num_reagents: int) -> list:
    """
    Convert results from object format to array format.

    Original: {"1": {"1": [...], "2": [...]}, "2": {...}}
    Output: [[[...], [...], None, ...], [[...], ...]]
    """
    output = []

    substance_keys = sorted(results_obj.keys(), key=lambda k: int(k))

    for substance_key in substance_keys:
        reagent_results = results_obj[substance_key]

        # Create list with slots for all reagents
        substance_output: list = [None] * num_reagents

        for reagent_key, result_list in reagent_results.items():
            reagent_idx = int(reagent_key) - 1

            if reagent_idx < num_reagents:
                converted_results = [
                    convert_result_entry(entry) for entry in result_list
                ]
                substance_output[reagent_idx] = converted_results

        output.append(substance_output)

    return output


def convert_reagents_json(input_path: Path, output_path: Path):
    """Convert from original format to our array format."""

    with open(input_path, "r") as f:
        data = json.load(f)

    num_colors = len(data["colors"])
    num_reagents = len(data["reagents"])
    num_substances = len(data["substances"])
    num_results = len(data["results"])

    print(f"Converting from: {input_path}")
    print(f"  Colors: {num_colors}")
    print(f"  Reagents: {num_reagents}")
    print(f"  Substances: {num_substances}")
    print(f"  Results: {num_results}")

    converted = {
        "colors": [convert_color(c) for c in convert_object_to_array(data["colors"])],
        "reagents": [
            convert_reagent(r) for r in convert_object_to_array(data["reagents"])
        ],
        "results": convert_results(data["results"], num_reagents),
        "substances": [
            convert_substance(s) for s in convert_object_to_array(data["substances"])
        ],
    }

    with open(output_path, "w") as f:
        json.dump(converted, f, indent=4)

    print(f"\nWrote converted data to: {output_path}")
    print(
        f"  Colors: {len(converted['colors'])} (IDs 0-{len(converted['colors']) - 1})"
    )
    print(
        f"  Reagents: {len(converted['reagents'])} (IDs 0-{len(converted['reagents']) - 1})"
    )
    print(f"  Substances: {len(converted['substances'])}")
    print(
        f"  Results: {len(converted['results'])} substances x {num_reagents} reagents"
    )


def main():
    script_dir = Path(__file__).parent
    project_dir = script_dir.parent

    input_path = project_dir / "data" / "reagents.orig.json"
    output_path = project_dir / "data" / "reagents.json"

    if len(sys.argv) >= 2:
        input_path = Path(sys.argv[1])
    if len(sys.argv) >= 3:
        output_path = Path(sys.argv[2])

    if not input_path.exists():
        print(f"Error: Input file not found: {input_path}")
        print(f"\nUsage: {sys.argv[0]} [input.json] [output.json]")
        print(f"  Default input:  data/reagents.orig.json")
        print(f"  Default output: data/reagents.json")
        sys.exit(1)

    convert_reagents_json(input_path, output_path)


if __name__ == "__main__":
    main()
