'use strict';

const gulp = require('gulp');
const eslint = require('gulp-eslint');

const paths = {
    scripts: [
        './**/*.js',
        '!./node_modules/**/*.js',
        '!./tools/**/*.js',
        '!./server/ac.js'
    ]
};

gulp.task('eslint', () => {
    return gulp.src(paths.scripts)
        .pipe(eslint())
        .pipe(eslint.format())
        .pipe(eslint.failAfterError());
});
