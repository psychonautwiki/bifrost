'use strict';

const RootQuery = `
type SubstanceClass {
    chemical: String
    psychoactive: String
}

type SubstanceTolerance {
    full: String
    half: String
    zero: String
}

interface RoaRange {
    min: Float
    max: Float
}

type SubstanceRoaRange implements RoaRange {
    min: Float
    max: Float
}

type SubstanceRoaDurationRange implements RoaRange {
    min: Float
    max: Float

    units: String
}

type SubstanceRoaDose {
    unit: String

    treshold: String
    heavy: String

    common: SubstanceRoaRange
    light: SubstanceRoaRange
    strong: SubstanceRoaRange
}

type SubstanceRoaDuration {
    afterglow: SubstanceRoaDurationRange
    comeup: SubstanceRoaDurationRange
    duration: SubstanceRoaDurationRange
    offset: SubstanceRoaDurationRange
    onset: SubstanceRoaDurationRange
    peak: SubstanceRoaDurationRange
    total: SubstanceRoaDurationRange
}

type SubstanceRoa {
    dose: SubstanceRoaDose
    duration: SubstanceRoaDuration
    bioavailability: SubstanceRoaRange
}

type SubstanceRoaTypes {
    buccal: SubstanceRoa
    inhalation: SubstanceRoa
    insufflation: SubstanceRoa
    intramuscular: SubstanceRoa
    intravenous: SubstanceRoa
    oral: SubstanceRoa
    rectal: SubstanceRoa
    smoked: SubstanceRoa
    sublingual: SubstanceRoa
    vaporized: SubstanceRoa
}

type Substance {
    name: String
    url: String

    featured: Boolean

	effects: [Effect]
	experiences: [Experience]

    class: SubstanceClass
    tolerance: SubstanceTolerance
    roa: SubstanceRoaTypes

    addictionPotential: String
    crossTolerance: [String]
    dangerousInteraction: [String]
}

type Effect {
    name: String
    url: String

	substances: [Substance]
	experiences: [Experience]
}

type Experience {
	substances: [Substance]
	effects: [Experience]
}

type Query {
    substances(
        # Name of the effect you want the substances of
        effect: String

        # Name of the substance you are looking for
    	query: String

        limit: Int=50
        Offset: Int=0
    ): [Substance]

    effects(
        # Name of the substance you want the effects of
    	substance: String

        # Name of the effect you are looking for
        query: String

        limit: Int=50
        Offset: Int=0
    ): [Effect]

    experiences(
    	effect: String,
    	substance: String
    ): [Experience]
}
`;

module.exports = () => [RootQuery];
